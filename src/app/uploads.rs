//! TIDAL Upload: the Upload page's list, the uploads in progress and the
//! dialogs' actions. The state is `crate::uploads::Uploads`; the requests
//! run in `crate::upload_jobs` on the backend's runtime.

use super::*;
use crate::backend::ApiResult;
use crate::upload_jobs::{UploadRequest, UploadResponse, Written};
use crate::uploads::{
    AlbumKey, AlbumType, CoverPick, EditDraft, FileProgress, PendingAlbum, PendingFile, Picked,
    Recheck, UploadAction, UploadDialog,
};

/// How often a release whose tracks are being scanned is read again while
/// its dialog is open, and for how long at most (TIDAL's web player polls
/// every second; this is gentler).
const RECHECK_EVERY: Duration = Duration::from_secs(3);
const RECHECK_FOR: Duration = Duration::from_secs(300);

/// The system's file dialog, on a thread of its own so the window keeps
/// drawing. `TIDALFAST_UPLOAD_DIALOG_PATH` (audio, paths joined by `;`) and
/// `TIDALFAST_UPLOAD_COVER_PATH` (a cover) answer it without one: automated
/// checks must never open a window on a screen.
fn pick_on_thread(
    ctx: egui::Context,
    slot: std::sync::Arc<std::sync::Mutex<Option<Picked>>>,
    cover: Option<Box<EditDraft>>,
    into: Option<AlbumKey>,
) {
    let _ = std::thread::Builder::new()
        .name("upload-dialog".into())
        .spawn(move || {
            let answer = if cover.is_some() {
                std::env::var_os("TIDALFAST_UPLOAD_COVER_PATH")
            } else {
                std::env::var_os("TIDALFAST_UPLOAD_DIALOG_PATH")
            };
            let chosen: Option<Vec<PathBuf>> = if let Some(answer) = answer {
                Some(
                    answer
                        .to_string_lossy()
                        .split(';')
                        .filter(|path| !path.trim().is_empty())
                        .map(PathBuf::from)
                        .collect(),
                )
            } else if cover.is_some() {
                rfd::FileDialog::new()
                    .set_title("Change image")
                    .add_filter("Image", &["jpg", "jpeg", "png"])
                    .pick_file()
                    .map(|path| vec![path])
            } else {
                rfd::FileDialog::new()
                    .set_title("Upload")
                    .add_filter("Audio", &crate::uploads::EXTENSIONS)
                    .pick_files()
            };
            let picked = match (chosen, cover) {
                (Some(files), Some(draft)) if !files.is_empty() => {
                    let path = files[0].clone();
                    let bytes = std::fs::metadata(&path)
                        .ok()
                        .filter(|meta| meta.len() <= 20 * 1024 * 1024)
                        .and_then(|_| std::fs::read(&path).ok())
                        .map(std::sync::Arc::<[u8]>::from);
                    Picked::Cover { draft, path, bytes }
                }
                (Some(files), None) if !files.is_empty() => Picked::Audio { files, into },
                (_, draft) => Picked::Nothing(draft),
            };
            *slot.lock().unwrap_or_else(|lock| lock.into_inner()) = Some(picked);
            ctx.request_repaint();
        });
}

impl App {
    /// Reads the list and the account's artist when the page opens.
    pub fn ensure_uploads(&mut self) {
        if !self.uploads.loading && (!self.uploads.loaded || self.uploads.error.is_some()) {
            self.reload_uploads();
        }
        if self.uploads.artist.is_none() && !self.uploads.artist_loading {
            self.uploads.artist_loading = true;
            self.backend.api(ApiRequest::Upload(UploadRequest::Artist));
        }
    }

    pub(super) fn reload_uploads(&mut self) {
        self.uploads.loading = true;
        self.uploads.error = None;
        self.backend
            .api(ApiRequest::Upload(UploadRequest::List { cursor: None }));
    }

    fn request_upload_tracks(&mut self, album_id: &str) {
        let list = self.uploads.tracks.entry(album_id.to_string()).or_default();
        if list.loading {
            return;
        }
        list.loading = true;
        self.backend.api(ApiRequest::Upload(UploadRequest::Tracks {
            album_id: album_id.to_string(),
        }));
    }

    /// The open release dialog's draft, when one is open.
    fn open_upload_draft(&mut self) -> Option<&mut EditDraft> {
        match self.dialog.as_mut() {
            Some(Dialog::Upload(dialog)) => match dialog.as_mut() {
                UploadDialog::Edit(draft) => Some(draft),
                UploadDialog::EditName { back, .. }
                | UploadDialog::TrackDetails { back, .. }
                | UploadDialog::ConfirmDeleteAlbum { back }
                | UploadDialog::ConfirmDeleteTrack { back, .. }
                | UploadDialog::MoveTrack { back, .. } => Some(back),
                _ => None,
            },
            _ => None,
        }
    }

    fn show_upload_dialog(&mut self, dialog: UploadDialog) {
        self.dialog = Some(Dialog::Upload(Box::new(dialog)));
    }

    /// Called once a frame: the file dialog's answer, and the next read of
    /// a release whose tracks are being scanned. Both cost a flag check
    /// while nothing is under way.
    pub(super) fn poll_uploads(&mut self, ctx: &egui::Context) {
        if self.uploads.picking {
            let picked = self
                .uploads
                .picked
                .lock()
                .unwrap_or_else(|lock| lock.into_inner())
                .take();
            if let Some(picked) = picked {
                self.uploads.picking = false;
                self.take_picked(picked, ctx);
            }
        }
        if let Some(recheck) = self.uploads.recheck.clone() {
            let open = self.open_upload_draft().map(|draft| draft.album.clone());
            let still_open = open
                .and_then(|key| self.uploads.album_id(&key))
                .is_some_and(|id| id == recheck.album_id);
            let now = Instant::now();
            if !still_open || now > recheck.until {
                self.uploads.recheck = None;
            } else if now >= recheck.at {
                self.uploads.recheck = None;
                self.request_upload_tracks(&recheck.album_id);
            } else {
                ctx.request_repaint_after(recheck.at - now);
            }
        }
    }

    fn take_picked(&mut self, picked: Picked, ctx: &egui::Context) {
        match picked {
            Picked::Audio { files, into } => match into {
                Some(key) => match self.uploads.album_id(&key) {
                    Some(album_id) => self.add_upload_files(files, album_id),
                    None => self.toast_error(gettext(
                        self.locale,
                        "The file is currently uploading. Wait until it's finished.",
                    )),
                },
                None => self.choose_upload_files(files),
            },
            Picked::Cover {
                mut draft,
                path,
                bytes,
            } => {
                if bytes.is_none() {
                    self.toast_error(gettext(self.locale, "Choose a JPEG or PNG image."));
                } else {
                    draft.new_cover = Some(path);
                    draft.suggested_pick = None;
                    draft.cover_preview = bytes;
                }
                self.show_upload_dialog(UploadDialog::Edit(draft));
            }
            Picked::Nothing(draft) => {
                if let Some(draft) = draft {
                    self.show_upload_dialog(UploadDialog::Edit(draft));
                }
            }
        }
        ctx.request_repaint();
    }

    /// The artist uploads go under, or a message why there is none.
    fn upload_artist_id(&mut self) -> Option<String> {
        match &self.uploads.artist {
            Some(Some(artist)) => Some(artist.id.clone()),
            Some(None) => {
                self.toast_error(gettext(
                    self.locale,
                    "Before uploading, let us know what name to release your music under.",
                ));
                None
            }
            None => {
                self.backend.api(ApiRequest::Upload(UploadRequest::Artist));
                self.toast_error(gettext(
                    self.locale,
                    "Your TIDAL artist profile isn't loaded yet. Try again in a moment.",
                ));
                None
            }
        }
    }

    /// Files chosen with "+ Upload": TIDAL's Organize dialog, or straight
    /// into a new single when it is one file and the account has no
    /// releases to add it to.
    fn choose_upload_files(&mut self, files: Vec<PathBuf>) {
        let (files, refused): (Vec<PathBuf>, Vec<PathBuf>) = files.into_iter().partition(|path| {
            crate::uploads::content_type(path).is_some_and(|kind| kind.starts_with("audio/"))
        });
        if !refused.is_empty() {
            self.toast_error(gettext(self.locale, "This file format isn't supported."));
        }
        if files.is_empty() || self.upload_artist_id().is_none() {
            return;
        }
        if files.len() == 1 && self.uploads.loaded && self.uploads.existing().is_empty() {
            self.upload_as_new(files);
        } else {
            self.show_upload_dialog(UploadDialog::Organize { files });
        }
    }

    /// "Upload as Single": the card shows at once with UPLOADING, its dialog
    /// opens, and the release is made; the files follow once it has an id.
    fn upload_as_new(&mut self, files: Vec<PathBuf>) {
        let Some(artist_id) = self.upload_artist_id() else {
            return;
        };
        let artist_name = match &self.uploads.artist {
            Some(Some(artist)) => artist.name.clone(),
            _ => String::new(),
        };
        let job = self.uploads.new_job();
        let title = if files.len() == 1 {
            crate::uploads::title_from_path(&files[0])
        } else {
            gettext(self.locale, "Untitled").into_owned()
        };
        let album_type = AlbumType::for_tracks(files.len());
        self.uploads.pending.push(PendingAlbum {
            job,
            id: None,
            title: title.clone(),
            album_type,
            artist_name,
            files: files.len() as u32,
            wanted: Default::default(),
            cover: None,
            delete_when_made: false,
            failed: false,
        });
        for (index, path) in files.into_iter().enumerate() {
            self.uploads.files.push(PendingFile {
                job,
                index,
                album: AlbumKey::Job(job),
                title: crate::uploads::title_from_path(&path),
                path,
                progress: FileProgress::Preparing,
            });
        }
        let key = AlbumKey::Job(job);
        self.uploads.dialog_serial += 1;
        if let Some(details) = self.uploads.album(&key) {
            self.show_upload_dialog(UploadDialog::Edit(Box::new(EditDraft::new(
                key, &details, true,
            ))));
        }
        self.backend
            .api(ApiRequest::Upload(UploadRequest::CreateAlbum {
                job,
                title,
                album_type,
                artist_id,
            }));
    }

    /// "Add to an existing album" > Add, and "Add tracks": the files go into
    /// that release; its dialog opens with them listed as Preparing.
    fn add_upload_files(&mut self, files: Vec<PathBuf>, album_id: String) {
        let Some(artist_id) = self.upload_artist_id() else {
            return;
        };
        let key = AlbumKey::Id(album_id.clone());
        let access = self
            .uploads
            .album(&key)
            .map(|album| album.access)
            .unwrap_or_default()
            .for_new_track();
        let job = self.uploads.new_job();
        for (index, path) in files.into_iter().enumerate() {
            let title = crate::uploads::title_from_path(&path);
            self.uploads.files.push(PendingFile {
                job,
                index,
                album: key.clone(),
                title: title.clone(),
                path: path.clone(),
                progress: FileProgress::Preparing,
            });
            self.backend.api(ApiRequest::Upload(UploadRequest::File {
                job,
                index,
                path,
                album_id: album_id.clone(),
                artist_id: artist_id.clone(),
                title,
                access,
            }));
        }
        self.open_upload_album(key);
    }

    pub(super) fn open_upload_album(&mut self, key: AlbumKey) {
        self.uploads.dialog_serial += 1;
        let Some(details) = self.uploads.album(&key) else {
            return;
        };
        if let Some(id) = self.uploads.album_id(&key) {
            self.request_upload_tracks(&id);
        }
        let draft = EditDraft::new(key, &details, false);
        self.show_upload_dialog(UploadDialog::Edit(Box::new(draft)));
    }

    fn write_upload(&mut self, request: UploadRequest) {
        self.backend.api(ApiRequest::Upload(request));
    }

    pub(super) fn handle_upload_action(&mut self, action: UploadAction, ctx: &egui::Context) {
        match action {
            UploadAction::Pick => {
                if !self.uploads.picking {
                    self.uploads.picking = true;
                    pick_on_thread(ctx.clone(), self.uploads.picked.clone(), None, None);
                }
            }
            UploadAction::Chosen(files) => self.choose_upload_files(files),
            UploadAction::AsSingle(files) => self.upload_as_new(files),
            UploadAction::ChooseExisting(files) => {
                self.show_upload_dialog(UploadDialog::AddToExisting {
                    files,
                    selected: None,
                });
            }
            UploadAction::AddTo { files, album_id } => self.add_upload_files(files, album_id),
            UploadAction::OpenAlbum(key) => self.open_upload_album(key),
            UploadAction::AddTracks(key) => {
                if !self.uploads.picking {
                    self.uploads.picking = true;
                    pick_on_thread(ctx.clone(), self.uploads.picked.clone(), None, Some(key));
                }
            }
            UploadAction::PickCover(draft) => {
                if !self.uploads.picking {
                    self.uploads.picking = true;
                    self.dialog = None;
                    pick_on_thread(ctx.clone(), self.uploads.picked.clone(), Some(draft), None);
                }
            }
            UploadAction::Suggest(mut draft) => match self.uploads.album_id(&draft.album) {
                Some(album_id) => {
                    draft.asking_suggestions = true;
                    self.show_upload_dialog(UploadDialog::Edit(draft));
                    self.write_upload(UploadRequest::Suggest { album_id });
                }
                None => self.toast(gettext(
                    self.locale,
                    "We're generating your spectrogram artwork. It should be ready soon.",
                )),
            },
            UploadAction::Save(draft) => self.save_upload_draft(*draft),
            UploadAction::RenameArtist {
                artist_id,
                name,
                back,
            } => {
                let name = name.trim().to_string();
                if !name.is_empty()
                    && let Some(Some(artist)) = &mut self.uploads.artist
                    && artist.name != name
                {
                    artist.name.clone_from(&name);
                    for album in &mut self.uploads.albums {
                        if album.artist_id.as_deref() == Some(artist_id.as_str()) {
                            album.artist_name.clone_from(&name);
                        }
                    }
                    for pending in &mut self.uploads.pending {
                        pending.artist_name.clone_from(&name);
                    }
                    self.write_upload(UploadRequest::RenameArtist { artist_id, name });
                }
                self.show_upload_dialog(UploadDialog::Edit(back));
            }
            UploadAction::RenameTrack {
                track_id,
                title,
                back,
            } => {
                let title = title.trim().to_string();
                if !title.is_empty() {
                    self.uploads
                        .track_titles
                        .insert(track_id.clone(), title.clone());
                    self.write_upload(UploadRequest::RenameTrack { track_id, title });
                }
                self.show_upload_dialog(UploadDialog::Edit(back));
            }
            UploadAction::MoveTrack {
                track_id,
                from,
                to,
                back,
            } => {
                self.uploads.forget_track(&track_id);
                let from_id = self.uploads.album_id(&from);
                for album in &mut self.uploads.albums {
                    if Some(&album.id) == from_id.as_ref() {
                        album.tracks = album.tracks.saturating_sub(1);
                    } else if album.id == to {
                        album.tracks += 1;
                    }
                }
                self.uploads.tracks.remove(&to);
                self.write_upload(UploadRequest::MoveTrack {
                    track_id,
                    album_id: to,
                });
                self.show_upload_dialog(UploadDialog::Edit(back));
            }
            UploadAction::Reorder {
                album_id,
                track_id,
                position,
            } => {
                if let Some(list) = self.uploads.tracks.get_mut(&album_id)
                    && let Some(from) = list.tracks.iter().position(|track| track.id == track_id)
                {
                    let track = list.tracks.remove(from);
                    let to = (position.max(1) as usize - 1).min(list.tracks.len());
                    list.tracks.insert(to, track);
                }
                self.write_upload(UploadRequest::Reorder {
                    album_id,
                    track_id,
                    position,
                });
            }
            UploadAction::DeleteTrack { track_id, back } => {
                self.uploads.forget_track(&track_id);
                if let Some(id) = self.uploads.album_id(&back.album)
                    && let Some(album) = self.uploads.albums.iter_mut().find(|a| a.id == id)
                {
                    album.tracks = album.tracks.saturating_sub(1);
                }
                self.write_upload(UploadRequest::DeleteTrack { track_id });
                self.show_upload_dialog(UploadDialog::Edit(back));
            }
            UploadAction::DeleteAlbum(key) => {
                self.dialog = None;
                match self.uploads.album_id(&key) {
                    Some(album_id) => {
                        self.uploads.deleted.insert(album_id.clone());
                        if let AlbumKey::Job(job) = key
                            && let Some(pending) =
                                self.uploads.pending.iter_mut().find(|p| p.job == job)
                        {
                            pending.failed = true;
                        }
                        self.write_upload(UploadRequest::DeleteAlbum { album_id });
                    }
                    None => {
                        if let AlbumKey::Job(job) = key
                            && let Some(pending) =
                                self.uploads.pending.iter_mut().find(|p| p.job == job)
                        {
                            pending.failed = true;
                            pending.delete_when_made = true;
                        }
                    }
                }
                self.toast(gettext(self.locale, "Upload deleted"));
            }
            UploadAction::ConfirmDeleteAlbum(key) => {
                if let Some(details) = self.uploads.album(&key) {
                    let draft = EditDraft::new(key, &details, false);
                    self.show_upload_dialog(UploadDialog::ConfirmDeleteAlbum {
                        back: Box::new(draft),
                    });
                }
            }
            // A release has no id until TIDAL answers; no link before that.
            UploadAction::CopyAlbumLink(key) => {
                if let Some(id) = self.uploads.album_id(&key) {
                    ctx.copy_text(format!("https://tidal.com/upload/{id}"));
                    self.toast(gettext(self.locale, "Link copied"));
                }
            }
            UploadAction::CopyTrackLink(track_id) => {
                ctx.copy_text(format!("https://tidal.com/track/{track_id}"));
                self.toast(gettext(self.locale, "Link copied"));
            }
            UploadAction::Back(draft) => self.show_upload_dialog(UploadDialog::Edit(draft)),
            UploadAction::Reload => self.reload_uploads(),
        }
    }

    /// Done: what changed shows at once (card and dialog) and is sent; a
    /// release still without an id keeps it for when it has one.
    fn save_upload_draft(&mut self, draft: EditDraft) {
        self.dialog = None;
        let change = draft.change();
        let cover = match (&draft.new_cover, &draft.suggested_pick) {
            (Some(path), _) => Some(CoverPick::File(path.clone())),
            (None, Some(pick)) => Some(CoverPick::Artwork(pick.artwork_id.clone())),
            _ => None,
        };
        let preview = cover.as_ref().and_then(|pick| match pick {
            CoverPick::File(_) => draft.cover_preview.as_ref().map(|bytes| {
                let uri = format!(
                    "bytes://upload-cover-{}-{}.img",
                    draft.title.len(),
                    bytes.len()
                );
                (uri, Some(bytes.clone()))
            }),
            CoverPick::Artwork(_) => draft
                .suggested_pick
                .as_ref()
                .and_then(|pick| pick.image.clone())
                .map(|url| (url, None)),
        });
        match self.uploads.album_id(&draft.album) {
            Some(album_id) => {
                if !change.is_empty() {
                    let edit = self.uploads.edits.entry(album_id.clone()).or_default();
                    crate::uploads::merge(edit, &change);
                    self.write_upload(UploadRequest::Change {
                        album_id: album_id.clone(),
                        change,
                    });
                }
                if let Some((uri, bytes)) = preview {
                    if let Some(bytes) = bytes {
                        self.uploads.cover_bytes.push((uri.clone(), bytes));
                    }
                    self.uploads.covers.insert(album_id.clone(), uri);
                }
                match cover {
                    Some(CoverPick::File(path)) => {
                        self.write_upload(UploadRequest::Cover { album_id, path })
                    }
                    Some(CoverPick::Artwork(artwork_id)) => {
                        self.write_upload(UploadRequest::SetCover {
                            album_id,
                            artwork_id,
                        })
                    }
                    None => {}
                }
            }
            None => {
                if let AlbumKey::Job(job) = draft.album
                    && let Some(pending) = self.uploads.pending.iter_mut().find(|p| p.job == job)
                {
                    crate::uploads::merge(&mut pending.wanted, &change);
                    if cover.is_some() {
                        pending.cover = cover;
                    }
                }
            }
        }
    }

    pub(super) fn handle_upload_response(&mut self, response: UploadResponse) {
        match response {
            UploadResponse::List { first, result } => match result {
                Ok(page) => {
                    let next = page.next.filter(|_| !page.items.is_empty());
                    self.uploads.absorb(first, page.items);
                    match next {
                        Some(cursor) => self.backend.api(ApiRequest::Upload(UploadRequest::List {
                            cursor: Some(cursor),
                        })),
                        None => self.uploads.loading = false,
                    }
                }
                Err(error) => {
                    self.uploads.loading = false;
                    if first {
                        self.uploads.error = Some(error.to_string());
                    }
                }
            },
            UploadResponse::Artist(result) => {
                self.uploads.artist_loading = false;
                match result {
                    Ok(artist) => self.uploads.artist = Some(artist),
                    Err(error) => log::debug!("TIDAL Upload artist unavailable: {error}"),
                }
            }
            UploadResponse::Tracks { album_id, result } => {
                let list = self.uploads.tracks.entry(album_id.clone()).or_default();
                list.loading = false;
                match result {
                    Ok(tracks) => {
                        list.tracks = tracks;
                        list.error = None;
                        let listed: HashSet<String> =
                            list.tracks.iter().map(|track| track.id.clone()).collect();
                        self.uploads.gone_tracks.retain(|id| listed.contains(id));
                        self.uploads.listed_tracks(&album_id);
                        let tracks = &self.uploads.tracks[&album_id].tracks;
                        self.uploads.track_titles.retain(|id, title| {
                            tracks
                                .iter()
                                .find(|track| &track.id == id)
                                .is_none_or(|track| &track.title != title)
                        });
                    }
                    Err(error) => list.error = Some(error.to_string()),
                }
                let key = AlbumKey::Id(album_id.clone());
                if self.uploads.scanning(&key) {
                    let now = Instant::now();
                    let until = match &self.uploads.recheck {
                        Some(recheck) if recheck.album_id == album_id => recheck.until,
                        _ => now + RECHECK_FOR,
                    };
                    self.uploads.recheck = Some(Recheck {
                        album_id,
                        at: now + RECHECK_EVERY,
                        until,
                    });
                }
            }
            UploadResponse::AlbumCreated { job, result } => self.upload_album_made(job, result),
            UploadResponse::Progress {
                job,
                index,
                percent,
            } => {
                if let Some(file) = self
                    .uploads
                    .files
                    .iter_mut()
                    .find(|file| file.job == job && file.index == index)
                    && file.busy()
                {
                    file.progress = FileProgress::Uploading(percent);
                }
            }
            UploadResponse::FileDone { job, index, result } => {
                self.upload_file_done(job, index, result)
            }
            UploadResponse::Suggested { album_id, result } => {
                let locale = self.locale;
                let enough = match &result {
                    Ok(covers) => covers.len() >= 9,
                    Err(_) => false,
                };
                let open = self
                    .open_upload_draft()
                    .map(|draft| draft.album.clone())
                    .unwrap_or(AlbumKey::Job(0));
                let matches = self.uploads.album_id(&open) == Some(album_id);
                if matches && let Some(draft) = self.open_upload_draft() {
                    draft.asking_suggestions = false;
                    if enough {
                        draft.suggestions = result.as_ref().ok().cloned();
                    }
                }
                match result {
                    Err(error) => self.toast_error(error.to_string()),
                    Ok(_) if !enough => self.toast(gettext(
                        locale,
                        "We're generating your spectrogram artwork. It should be ready soon.",
                    )),
                    Ok(_) => {}
                }
            }
            UploadResponse::Written {
                what,
                request,
                result,
            } => self.upload_written(what, *request, result),
        }
    }

    fn upload_album_made(&mut self, job: u64, result: ApiResult<String>) {
        let Some(pending) = self.uploads.pending.iter_mut().find(|p| p.job == job) else {
            return;
        };
        match result {
            Ok(album_id) => {
                pending.id = Some(album_id.clone());
                self.uploads.made.insert(job, album_id.clone());
                if pending.delete_when_made {
                    self.write_upload(UploadRequest::DeleteAlbum { album_id });
                    for file in &mut self.uploads.files {
                        if file.job == job {
                            file.progress = FileProgress::Failed(String::new());
                        }
                    }
                    return;
                }
                let wanted = std::mem::take(&mut pending.wanted);
                let cover = pending.cover.take();
                if !wanted.is_empty() {
                    let edit = self.uploads.edits.entry(album_id.clone()).or_default();
                    crate::uploads::merge(edit, &wanted);
                    self.write_upload(UploadRequest::Change {
                        album_id: album_id.clone(),
                        change: wanted,
                    });
                }
                match cover {
                    Some(CoverPick::File(path)) => self.write_upload(UploadRequest::Cover {
                        album_id: album_id.clone(),
                        path,
                    }),
                    Some(CoverPick::Artwork(artwork_id)) => {
                        self.write_upload(UploadRequest::SetCover {
                            album_id: album_id.clone(),
                            artwork_id,
                        })
                    }
                    None => {}
                }
                let Some(artist_id) = self.upload_artist_id() else {
                    return;
                };
                let files: Vec<PendingFile> = self
                    .uploads
                    .files
                    .iter()
                    .filter(|file| file.job == job)
                    .cloned()
                    .collect();
                for file in files {
                    self.backend.api(ApiRequest::Upload(UploadRequest::File {
                        job,
                        index: file.index,
                        path: file.path,
                        album_id: album_id.clone(),
                        artist_id: artist_id.clone(),
                        title: file.title,
                        access: crate::uploads::Access::Unlisted,
                    }));
                }
            }
            Err(error) => {
                pending.failed = true;
                for file in &mut self.uploads.files {
                    if file.job == job {
                        file.progress = FileProgress::Failed(error.to_string());
                    }
                }
                if matches!(
                    self.open_upload_draft().map(|draft| &draft.album),
                    Some(AlbumKey::Job(open)) if *open == job
                ) {
                    self.dialog = None;
                }
                self.toast_error(gettext(self.locale, "Upload failed. Please try again."));
            }
        }
    }

    fn upload_file_done(&mut self, job: u64, index: usize, result: ApiResult<String>) {
        let Some(file) = self
            .uploads
            .files
            .iter_mut()
            .find(|file| file.job == job && file.index == index)
        else {
            return;
        };
        let album = file.album.clone();
        match result {
            Ok(track_id) => file.progress = FileProgress::Sent { track_id },
            Err(error) => {
                log::warn!("upload failed: {error}");
                file.progress = FileProgress::Failed(error.to_string());
                let message = match &error {
                    crate::api::ApiError::Decode(reason) if !reason.contains(' ') => {
                        gettext(self.locale, "Upload failed. Please try again.").into_owned()
                    }
                    crate::api::ApiError::Decode(reason) => reason.clone(),
                    _ => gettext(self.locale, "Upload failed. Please try again.").into_owned(),
                };
                self.toast_error(message);
            }
        }
        if self.uploads.uploading(&album) {
            return;
        }
        let all_sent = self
            .uploads
            .files
            .iter()
            .filter(|f| f.job == job)
            .all(|f| matches!(f.progress, FileProgress::Sent { .. }));
        if all_sent {
            self.toast(gettext(self.locale, "Upload complete"));
        }
        // Every file of the release has finished: a new release none of
        // whose files went is removed again, as TIDAL's web player does.
        if let AlbumKey::Job(job) = album {
            let none_sent = self
                .uploads
                .files
                .iter()
                .filter(|f| f.job == job)
                .all(|f| matches!(f.progress, FileProgress::Failed(_)));
            if none_sent
                && let Some(pending) = self.uploads.pending.iter_mut().find(|p| p.job == job)
            {
                pending.failed = true;
                if let Some(album_id) = pending.id.clone() {
                    self.write_upload(UploadRequest::DeleteAlbum { album_id });
                }
            }
        }
        if let Some(album_id) = self.uploads.album_id(&album) {
            self.request_upload_tracks(&album_id);
        }
        self.reload_uploads();
    }

    fn upload_written(&mut self, what: Written, request: UploadRequest, result: ApiResult<()>) {
        if let Err(error) = result {
            log::warn!("TIDAL Upload write failed: {error}");
            match (&what, &request) {
                (Written::Album { album_id }, UploadRequest::Change { .. }) => {
                    self.uploads.edits.remove(album_id);
                }
                (Written::Album { album_id }, _) => {
                    self.uploads.covers.remove(album_id);
                }
                (Written::Deleted { album_id }, _) => {
                    self.uploads.deleted.remove(album_id);
                }
                (Written::Track { track_id }, UploadRequest::RenameTrack { .. }) => {
                    self.uploads.track_titles.remove(track_id);
                }
                (Written::Track { track_id }, _) => {
                    self.uploads.gone_tracks.remove(track_id);
                }
                (Written::Artist, _) => self.uploads.artist = None,
            }
            self.toast_error(
                gettext(self.locale, "Couldn't save your changes. Try again.").into_owned(),
            );
            if matches!(what, Written::Artist) {
                self.backend.api(ApiRequest::Upload(UploadRequest::Artist));
            }
        }
        // TIDAL's state is read again behind what is shown.
        match &request {
            UploadRequest::MoveTrack { album_id, .. } => {
                self.uploads.tracks.remove(album_id);
                if let Some(open) = self
                    .open_upload_draft()
                    .map(|draft| draft.album.clone())
                    .and_then(|key| self.uploads.album_id(&key))
                {
                    self.request_upload_tracks(&open);
                }
            }
            UploadRequest::DeleteTrack { .. }
            | UploadRequest::RenameTrack { .. }
            | UploadRequest::Reorder { .. } => {
                if let Some(open) = self
                    .open_upload_draft()
                    .map(|draft| draft.album.clone())
                    .and_then(|key| self.uploads.album_id(&key))
                {
                    self.request_upload_tracks(&open);
                }
            }
            _ => {}
        }
        self.reload_uploads();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::uploads::{Access, AlbumChange, UploadAlbum, UploadArtist};

    fn app(name: &str) -> App {
        let root = crate::paths::unique_test_dir(&format!("uploads-{name}"));
        let _ = std::fs::remove_dir_all(&root);
        let mut app = App::new(
            &Waker::default(),
            AppDirs {
                config: root.join("config"),
                state: root.join("state"),
                cache: root.join("cache"),
            },
            Settings::default(),
            AppOptions {
                media_controls: false,
                restore_sign_in: false,
                tray: false,
            },
        );
        app.network_reachable = true;
        app.uploads.artist = Some(Some(UploadArtist {
            id: "77".into(),
            name: "Me".into(),
        }));
        app.uploads.absorb(
            true,
            vec![UploadAlbum {
                id: "500".into(),
                title: "Old".into(),
                artist_id: Some("77".into()),
                artist_name: "Me".into(),
                tracks: 1,
                access: Access::Public,
                ..Default::default()
            }],
        );
        app.backend.take_upload_requests();
        app
    }

    fn file() -> Vec<PathBuf> {
        vec![PathBuf::from("C:/music/Tidalfast test.wav")]
    }

    /// Upload as Single: the card shows UPLOADING and the dialog opens
    /// before TIDAL has answered; the file follows the album's id, and a
    /// title chosen meanwhile is sent once the album exists.
    #[test]
    fn a_new_single_shows_at_once_and_its_file_follows_the_album() {
        let mut app = app("single");
        let ctx = egui::Context::default();
        app.handle_upload_action(UploadAction::Chosen(file()), &ctx);
        assert!(
            matches!(&app.dialog, Some(Dialog::Upload(d)) if matches!(**d, UploadDialog::Organize { .. })),
            "with releases to add to, Organize asks first"
        );
        app.handle_upload_action(UploadAction::AsSingle(file()), &ctx);
        let cards = app.uploads.cards();
        assert_eq!(cards[0].title, "Tidalfast test");
        assert!(cards[0].uploading);
        let Some(Dialog::Upload(dialog)) = app.dialog.clone() else {
            panic!("the New single dialog opens at once");
        };
        let UploadDialog::Edit(mut draft) = *dialog else {
            panic!("not the release dialog");
        };
        assert!(draft.is_new);
        let requests = app.backend.take_upload_requests();
        assert!(matches!(
            requests.as_slice(),
            [UploadRequest::CreateAlbum { title, album_type: AlbumType::Single, artist_id, .. }]
                if title == "Tidalfast test" && artist_id == "77"
        ));
        let UploadRequest::CreateAlbum { job, .. } = requests[0].clone() else {
            unreachable!()
        };
        // Done before TIDAL answered: kept for the album.
        draft.title = "Renamed".into();
        app.handle_upload_action(UploadAction::Save(draft), &ctx);
        assert!(app.backend.take_upload_requests().is_empty());
        assert_eq!(app.uploads.cards()[0].title, "Renamed");
        app.handle_api(ApiResponse::Upload(UploadResponse::AlbumCreated {
            job,
            result: Ok("900".into()),
        }));
        let requests = app.backend.take_upload_requests();
        assert!(requests.iter().any(|r| matches!(r,
            UploadRequest::Change { album_id, change }
                if album_id == "900" && change.title.as_deref() == Some("Renamed"))));
        assert!(requests.iter().any(|r| matches!(r,
            UploadRequest::File { album_id, access: Access::Unlisted, title, .. }
                if album_id == "900" && title == "Tidalfast test")));
        // The file went: the list and the release's tracks are read again,
        // and a list that lags keeps the card.
        app.handle_api(ApiResponse::Upload(UploadResponse::FileDone {
            job,
            index: 0,
            result: Ok("t1".into()),
        }));
        let requests = app.backend.take_upload_requests();
        assert!(requests.contains(&UploadRequest::Tracks {
            album_id: "900".into()
        }));
        assert!(requests.contains(&UploadRequest::List { cursor: None }));
        app.handle_api(ApiResponse::Upload(UploadResponse::List {
            first: true,
            result: Ok(crate::api::tidal::uploads::Paged {
                items: app.uploads.albums.clone(),
                next: None,
            }),
        }));
        let cards = app.uploads.cards();
        assert_eq!(cards.len(), 2);
        assert_eq!(cards[0].title, "Renamed");
        assert!(!cards[0].uploading);
        // TIDAL lists it now; the dialog opened while it was being made
        // still sends its changes to it (live test: Done sent nothing).
        let mut listed = app.uploads.albums.clone();
        listed.insert(
            0,
            UploadAlbum {
                id: "900".into(),
                title: "Renamed".into(),
                tracks: 1,
                ..Default::default()
            },
        );
        app.handle_api(ApiResponse::Upload(UploadResponse::List {
            first: true,
            result: Ok(crate::api::tidal::uploads::Paged {
                items: listed,
                next: None,
            }),
        }));
        app.backend.take_upload_requests();
        let mut draft = EditDraft::new(
            AlbumKey::Job(job),
            &app.uploads.album(&AlbumKey::Job(job)).unwrap(),
            true,
        );
        draft.album_type = AlbumType::Ep;
        app.handle_upload_action(UploadAction::Save(Box::new(draft)), &ctx);
        assert!(
            app.backend
                .take_upload_requests()
                .iter()
                .any(|r| matches!(r,
            UploadRequest::Change { album_id, change }
                if album_id == "900" && change.album_type == Some(AlbumType::Ep)))
        );
        app.backend.shutdown();
    }

    /// Copy link puts the release's address on the clipboard; a release
    /// without an id yet has none.
    #[test]
    fn copy_link_gives_the_upload_address() {
        let mut app = app("copy-link");
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            app.handle_upload_action(
                UploadAction::CopyAlbumLink(AlbumKey::Id("500".into())),
                ui.ctx(),
            );
        });
        output.textures_delta.clear();
        assert!(
            output
                .platform_output
                .commands
                .contains(&egui::OutputCommand::CopyText(
                    "https://tidal.com/upload/500".into()
                ))
        );
        assert!(app.toasts.iter().any(|t| t.message == "Link copied"));
        let toasts = app.toasts.len();
        app.handle_upload_action(UploadAction::CopyAlbumLink(AlbumKey::Job(9)), &ctx);
        assert_eq!(app.toasts.len(), toasts);
        app.backend.shutdown();
    }

    /// The menu's Delete album only asks; the request goes when confirmed.
    #[test]
    fn card_delete_asks_before_deleting() {
        let mut app = app("card-delete");
        let ctx = egui::Context::default();
        app.handle_upload_action(
            UploadAction::ConfirmDeleteAlbum(AlbumKey::Id("500".into())),
            &ctx,
        );
        assert!(matches!(
            &app.dialog,
            Some(Dialog::Upload(dialog))
                if matches!(**dialog, UploadDialog::ConfirmDeleteAlbum { .. })
        ));
        assert!(app.backend.take_upload_requests().is_empty());
        assert_eq!(app.uploads.cards().len(), 1);
        app.backend.shutdown();
    }

    /// A pasted `tidal.com/upload/<id>` link opens the Upload page, and
    /// the release's dialog when it is listed.
    #[test]
    fn an_upload_link_opens_the_upload_page() {
        let mut app = app("upload-link");
        app.user = Some(crate::api::models::User {
            id: "me".into(),
            ..Default::default()
        });
        let ctx = egui::Context::default();
        let uri = crate::link::parse("https://tidal.com/upload/500").unwrap();
        app.open_link(uri);
        app.apply_actions(&ctx);
        app.open_pending_link();
        app.apply_actions(&ctx);
        assert_eq!(*app.page(), Page::Uploads);
        assert!(matches!(
            &app.dialog,
            Some(Dialog::Upload(dialog)) if matches!(**dialog, UploadDialog::Edit(_))
        ));
        app.backend.shutdown();
    }

    /// A deleted release is gone at once; a failed delete brings it back.
    #[test]
    fn delete_hides_at_once_and_a_failure_restores() {
        let mut app = app("delete");
        let ctx = egui::Context::default();
        app.handle_upload_action(UploadAction::DeleteAlbum(AlbumKey::Id("500".into())), &ctx);
        assert!(app.uploads.cards().is_empty());
        let requests = app.backend.take_upload_requests();
        assert_eq!(
            requests,
            vec![UploadRequest::DeleteAlbum {
                album_id: "500".into()
            }]
        );
        app.handle_api(ApiResponse::Upload(UploadResponse::Written {
            what: Written::Deleted {
                album_id: "500".into(),
            },
            request: Box::new(requests[0].clone()),
            result: Err(crate::api::ApiError::Network("down".into())),
        }));
        assert_eq!(app.uploads.cards().len(), 1);
        assert!(app.toasts.iter().any(|t| t.kind == ToastKind::Error));
        app.backend.shutdown();
    }

    /// Done sends only the fields that changed, and the card shows them.
    #[test]
    fn done_sends_only_changes_and_shows_them() {
        let mut app = app("done");
        let ctx = egui::Context::default();
        app.handle_upload_action(UploadAction::OpenAlbum(AlbumKey::Id("500".into())), &ctx);
        assert!(
            app.backend
                .take_upload_requests()
                .contains(&UploadRequest::Tracks {
                    album_id: "500".into()
                })
        );
        let Some(Dialog::Upload(dialog)) = app.dialog.clone() else {
            panic!("the dialog opens");
        };
        let UploadDialog::Edit(mut draft) = *dialog else {
            panic!("not the release dialog");
        };
        draft.album_type = AlbumType::Ep;
        app.handle_upload_action(UploadAction::Save(draft), &ctx);
        assert_eq!(
            app.backend.take_upload_requests(),
            vec![UploadRequest::Change {
                album_id: "500".into(),
                change: AlbumChange {
                    album_type: Some(AlbumType::Ep),
                    ..Default::default()
                },
            }]
        );
        assert_eq!(
            app.uploads
                .album(&AlbumKey::Id("500".into()))
                .unwrap()
                .album_type,
            AlbumType::Ep
        );
        app.backend.shutdown();
    }

    /// A file TIDAL does not take is refused before anything is sent.
    #[test]
    fn unsupported_files_are_refused_here() {
        let mut app = app("refuse");
        let ctx = egui::Context::default();
        app.handle_upload_action(UploadAction::Chosen(vec![PathBuf::from("notes.txt")]), &ctx);
        assert!(app.dialog.is_none());
        assert!(app.backend.take_upload_requests().is_empty());
        assert!(app.toasts.iter().any(|t| t.kind == ToastKind::Error));
        app.backend.shutdown();
    }

    /// Opening the page reads the list and the artist once.
    #[test]
    fn the_page_reads_its_list_once() {
        let mut app = app("open");
        app.uploads.loaded = false;
        app.uploads.artist = None;
        app.ensure_uploads();
        app.ensure_uploads();
        let requests = app.backend.take_upload_requests();
        assert_eq!(
            requests
                .iter()
                .filter(|r| matches!(r, UploadRequest::List { .. }))
                .count(),
            1
        );
        assert!(requests.contains(&UploadRequest::Artist));
        app.backend.shutdown();
    }
}
