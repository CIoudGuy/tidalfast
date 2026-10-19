//! TIDAL Upload: the account's uploaded releases as the Upload page shows
//! them, the uploads in progress, and the dialogs' drafts. The interface
//! is optimistic: a release being made shows its card with "UPLOADING" the
//! moment its files are chosen, an edit shows at once, a deleted release is
//! gone at once, and TIDAL's answers, which can lag, never undo them.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub use crate::api::tidal::uploads::{
    Access, AlbumChange, AlbumType, FileState, SuggestedCover, UploadAlbum, UploadArtist,
    UploadTrack,
};

/// TIDAL's largest upload: 5 GB.
pub const MAX_FILE: u64 = 5 * 1024 * 1024 * 1024;

/// The audio files TIDAL's upload takes (its file picker's `accept`).
pub const EXTENSIONS: [&str; 9] = [
    "flac", "wav", "aif", "aiff", "mp3", "m4a", "mp4", "aac", "ogg",
];

/// The `Content-Type` a file is sent with, from its extension; `None` for
/// a file TIDAL does not take.
pub fn content_type(path: &Path) -> Option<&'static str> {
    let extension = path.extension()?.to_str()?.to_ascii_lowercase();
    Some(match extension.as_str() {
        "flac" => "audio/flac",
        "wav" => "audio/wav",
        "aif" | "aiff" => "audio/aiff",
        "mp3" => "audio/mpeg",
        "m4a" | "mp4" | "aac" => "audio/mp4",
        "ogg" => "audio/ogg",
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        _ => return None,
    })
}

/// A track's title from its file: the name without its extension.
pub fn title_from_path(path: &Path) -> String {
    let stem = path
        .file_stem()
        .map(|stem| stem.to_string_lossy().trim().to_string())
        .unwrap_or_default();
    if stem.is_empty() {
        "Untitled".into()
    } else {
        stem.chars().take(255).collect()
    }
}

/// Which release something belongs to: one being made (known only by its
/// job until TIDAL answers with an id) or one TIDAL knows.
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum AlbumKey {
    Job(u64),
    Id(String),
}

/// Where one chosen file is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FileProgress {
    /// Hashing and announcing it to TIDAL.
    Preparing,
    /// Sending: percent done.
    Uploading(u8),
    /// Sent; TIDAL processes and scans it now.
    Sent {
        track_id: String,
    },
    Failed(String),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PendingFile {
    pub job: u64,
    pub index: usize,
    pub album: AlbumKey,
    pub title: String,
    pub path: PathBuf,
    pub progress: FileProgress,
}

impl PendingFile {
    pub fn busy(&self) -> bool {
        matches!(
            self.progress,
            FileProgress::Preparing | FileProgress::Uploading(_)
        )
    }
}

/// A release being made: its card shows before TIDAL lists it.
#[derive(Clone, Debug, PartialEq)]
pub struct PendingAlbum {
    pub job: u64,
    pub id: Option<String>,
    pub title: String,
    pub album_type: AlbumType,
    pub artist_name: String,
    pub files: u32,
    /// Details chosen in its dialog before TIDAL gave it an id.
    pub wanted: AlbumChange,
    /// A cover chosen before TIDAL gave it an id.
    pub cover: Option<CoverPick>,
    /// Deleted before TIDAL gave it an id: deleted as soon as it has one.
    pub delete_when_made: bool,
    pub failed: bool,
}

/// A cover chosen in a release's dialog.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CoverPick {
    File(PathBuf),
    Artwork(String),
}

/// What the system's file dialog answered.
#[derive(Clone, Debug)]
pub enum Picked {
    /// Audio files, for a new release or into `into`.
    Audio {
        files: Vec<PathBuf>,
        into: Option<AlbumKey>,
    },
    /// A cover image for the draft, with its bytes for the preview.
    Cover {
        draft: Box<EditDraft>,
        path: PathBuf,
        bytes: Option<std::sync::Arc<[u8]>>,
    },
    /// Closed without a choice; the draft comes back.
    Nothing(Option<Box<EditDraft>>),
}

/// A release's tracks as its dialog lists them.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TrackList {
    pub tracks: Vec<UploadTrack>,
    pub loading: bool,
    pub error: Option<String>,
}

/// One card of the Upload page.
#[derive(Clone, Debug, PartialEq)]
pub struct Card {
    pub key: AlbumKey,
    pub title: String,
    pub artist_name: String,
    pub cover: Option<String>,
    pub access: Access,
    pub tracks: u32,
    pub uploading: bool,
}

#[derive(Debug, Default)]
pub struct Uploads {
    pub albums: Vec<UploadAlbum>,
    pub loaded: bool,
    pub loading: bool,
    pub error: Option<String>,
    /// The account's artist; `None` until read, `Some(None)` when it has none.
    pub artist: Option<Option<UploadArtist>>,
    pub artist_loading: bool,
    pub pending: Vec<PendingAlbum>,
    pub files: Vec<PendingFile>,
    pub tracks: HashMap<String, TrackList>,
    /// Deleted here; hidden until TIDAL's list no longer has them.
    pub deleted: HashSet<String>,
    /// Edited here; shown over TIDAL's list until it agrees.
    pub edits: HashMap<String, AlbumChange>,
    /// Tracks deleted or moved away here, hidden from their old release.
    pub gone_tracks: HashSet<String>,
    /// Track titles changed here, shown until TIDAL agrees.
    pub track_titles: HashMap<String, String>,
    /// The system file dialog is open (for audio, or a cover).
    pub picking: bool,
    /// Where the file dialog's thread leaves its answer.
    pub picked: std::sync::Arc<std::sync::Mutex<Option<Picked>>>,
    /// Covers chosen here, by release id (an image the app holds),
    /// shown on the card until TIDAL lists a cover.
    pub covers: HashMap<String, String>,
    /// Picked cover images the window has yet to take in (by their `bytes://`
    /// address), handed over once.
    pub cover_bytes: Vec<(String, std::sync::Arc<[u8]>)>,
    /// A release whose tracks are being scanned is read again at this time
    /// while its dialog is open; `until` ends the checks.
    pub recheck: Option<Recheck>,
    /// The id TIDAL gave each release made here, by job: a dialog opened
    /// on the release while it was being made still finds it once TIDAL
    /// lists it and its card is TIDAL's own.
    pub made: HashMap<u64, String>,
    /// Bumped each time a release's dialog opens, so it opens at its top.
    pub dialog_serial: u64,
    /// Demo builds: a dropdown or menu shown open in the first frame.
    pub demo_popup: Option<egui::Id>,
    next_job: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Recheck {
    pub album_id: String,
    pub at: std::time::Instant,
    pub until: std::time::Instant,
}

impl Uploads {
    pub fn new_job(&mut self) -> u64 {
        self.next_job += 1;
        self.next_job
    }

    /// The release a key names, as known now: its id once TIDAL gave one.
    pub fn album_id(&self, key: &AlbumKey) -> Option<String> {
        match key {
            AlbumKey::Id(id) => Some(id.clone()),
            AlbumKey::Job(job) => self.made.get(job).cloned().or_else(|| {
                self.pending
                    .iter()
                    .find(|pending| pending.job == *job)
                    .and_then(|pending| pending.id.clone())
            }),
        }
    }

    /// The same release by whichever key it is known under.
    pub fn same_album(&self, a: &AlbumKey, b: &AlbumKey) -> bool {
        a == b
            || self
                .album_id(a)
                .is_some_and(|id| self.album_id(b).as_deref() == Some(id.as_str()))
    }

    /// A track deleted or moved away here: its file, if it was sent from
    /// this window, is no longer one of its old release's rows.
    pub fn forget_track(&mut self, track_id: &str) {
        self.gone_tracks.insert(track_id.to_string());
        self.files.retain(|file| {
            !matches!(&file.progress, FileProgress::Sent { track_id: sent } if sent == track_id)
        });
    }

    /// TIDAL listed a release's tracks: files sent from here that it lists
    /// are its rows now.
    pub fn listed_tracks(&mut self, album_id: &str) {
        let Some(list) = self.tracks.get(album_id) else {
            return;
        };
        let listed: HashSet<&str> = list.tracks.iter().map(|track| track.id.as_str()).collect();
        let made = &self.made;
        let pending = &self.pending;
        let id_of = |key: &AlbumKey| match key {
            AlbumKey::Id(id) => Some(id.clone()),
            AlbumKey::Job(job) => made.get(job).cloned().or_else(|| {
                pending
                    .iter()
                    .find(|p| p.job == *job)
                    .and_then(|p| p.id.clone())
            }),
        };
        self.files.retain(|file| match &file.progress {
            FileProgress::Sent { track_id } => {
                id_of(&file.album).as_deref() != Some(album_id)
                    || !listed.contains(track_id.as_str())
            }
            _ => true,
        });
    }

    pub fn uploading(&self, key: &AlbumKey) -> bool {
        self.files
            .iter()
            .any(|file| file.busy() && self.same_album(&file.album, key))
    }

    /// TIDAL's list as it arrived. Releases made here that it does not list
    /// yet keep their cards; deleted ones stay hidden until it drops them;
    /// edits it agrees with are let go.
    pub fn absorb(&mut self, first: bool, albums: Vec<UploadAlbum>) {
        if first {
            self.albums = albums;
        } else {
            for album in albums {
                if !self.albums.iter().any(|known| known.id == album.id) {
                    self.albums.push(album);
                }
            }
        }
        self.loaded = true;
        self.error = None;
        let listed: HashSet<&str> = self.albums.iter().map(|album| album.id.as_str()).collect();
        self.deleted.retain(|id| listed.contains(id.as_str()));
        let files = &self.files;
        self.pending.retain(|pending| {
            let busy = files
                .iter()
                .any(|file| file.busy() && file.album == AlbumKey::Job(pending.job));
            busy || pending.id.as_deref().is_none_or(|id| !listed.contains(id))
        });
        let albums = &self.albums;
        self.edits.retain(|id, change| {
            albums
                .iter()
                .find(|album| &album.id == id)
                .is_none_or(|album| !agrees(album, change))
        });
    }

    /// The cards, newest first: releases being made, then TIDAL's list.
    pub fn cards(&self) -> Vec<Card> {
        let mut cards: Vec<Card> = self
            .pending
            .iter()
            .rev()
            .filter(|pending| !pending.failed)
            .filter(|pending| {
                pending
                    .id
                    .as_deref()
                    .is_none_or(|id| !self.albums.iter().any(|album| album.id == id))
            })
            .filter(|pending| {
                pending
                    .id
                    .as_deref()
                    .is_none_or(|id| !self.deleted.contains(id))
            })
            .map(|pending| {
                let key = AlbumKey::Job(pending.job);
                let mut title = pending.title.clone();
                if let Some(wanted) = &pending.wanted.title {
                    title.clone_from(wanted);
                }
                if let Some(id) = &pending.id
                    && let Some(edit) = self.edits.get(id)
                    && let Some(edited) = &edit.title
                {
                    title.clone_from(edited);
                }
                Card {
                    uploading: self.uploading(&key),
                    key,
                    title,
                    artist_name: pending.artist_name.clone(),
                    cover: None,
                    access: pending.wanted.access.unwrap_or_default(),
                    tracks: pending.files,
                }
            })
            .collect();
        for album in &self.albums {
            if self.deleted.contains(&album.id) {
                continue;
            }
            let edit = self.edits.get(&album.id);
            let key = AlbumKey::Id(album.id.clone());
            let added = self
                .files
                .iter()
                .filter(|file| file.busy() && file.album == key)
                .count() as u32;
            cards.push(Card {
                uploading: self.uploading(&key),
                title: edit
                    .and_then(|edit| edit.title.clone())
                    .unwrap_or_else(|| album.title.clone()),
                artist_name: album.artist_name.clone(),
                cover: self
                    .covers
                    .get(&album.id)
                    .cloned()
                    .or_else(|| album.cover.clone()),
                access: edit.and_then(|edit| edit.access).unwrap_or(album.access),
                tracks: album.tracks + added,
                key,
            });
        }
        cards
    }

    /// A release's details as shown: TIDAL's, with edits made here.
    pub fn album(&self, key: &AlbumKey) -> Option<UploadAlbum> {
        let id = self.album_id(key);
        let mut album = id
            .as_deref()
            .and_then(|id| self.albums.iter().find(|album| album.id == id))
            .cloned()
            .or_else(|| {
                let AlbumKey::Job(job) = key else {
                    return None;
                };
                let pending = self.pending.iter().find(|pending| pending.job == *job)?;
                Some(UploadAlbum {
                    id: pending.id.clone().unwrap_or_default(),
                    title: pending.title.clone(),
                    album_type: pending.album_type,
                    artist_name: pending.artist_name.clone(),
                    tracks: pending.files,
                    ..Default::default()
                })
            })?;
        if let AlbumKey::Job(job) = key
            && let Some(pending) = self.pending.iter().find(|pending| pending.job == *job)
        {
            apply(&mut album, &pending.wanted);
        }
        if let Some(edit) = id.as_deref().and_then(|id| self.edits.get(id)) {
            apply(&mut album, edit);
        }
        if let Some(cover) = id.as_deref().and_then(|id| self.covers.get(id)) {
            album.cover = Some(cover.clone());
        }
        Some(album)
    }

    /// The tracks a release's dialog lists: TIDAL's (less those deleted or
    /// moved away here), then the files still on their way.
    pub fn rows(&self, key: &AlbumKey) -> Vec<Row> {
        let mut rows: Vec<Row> = self
            .album_id(key)
            .and_then(|id| self.tracks.get(&id))
            .map(|list| {
                list.tracks
                    .iter()
                    .filter(|track| !self.gone_tracks.contains(&track.id))
                    .map(|track| Row {
                        track_id: Some(track.id.clone()),
                        title: self
                            .track_titles
                            .get(&track.id)
                            .cloned()
                            .unwrap_or_else(|| track.title.clone()),
                        access: track.access,
                        duration_secs: track.duration_secs,
                        state: match track.file {
                            FileState::Ready if track.duration_secs.is_some() => RowState::Ready,
                            FileState::Scanning => RowState::Scanning,
                            FileState::Flagged => RowState::Flagged,
                            FileState::Failed => RowState::Failed,
                            _ => RowState::Preparing,
                        },
                    })
                    .collect()
            })
            .unwrap_or_default();
        for file in &self.files {
            if !self.same_album(&file.album, key) {
                continue;
            }
            let listed = match &file.progress {
                FileProgress::Sent { track_id } => rows
                    .iter()
                    .any(|row| row.track_id.as_deref() == Some(track_id)),
                _ => false,
            };
            if listed {
                continue;
            }
            rows.push(Row {
                track_id: match &file.progress {
                    FileProgress::Sent { track_id } => Some(track_id.clone()),
                    _ => None,
                },
                title: file.title.clone(),
                access: Access::Unlisted,
                duration_secs: None,
                state: match &file.progress {
                    FileProgress::Failed(_) => RowState::Failed,
                    FileProgress::Uploading(percent) => RowState::Uploading(*percent),
                    _ => RowState::Preparing,
                },
            });
        }
        rows
    }

    /// Any of the release's tracks still uploading or being scanned: its
    /// visibility waits (TIDAL's rule).
    pub fn scanning(&self, key: &AlbumKey) -> bool {
        self.rows(key).iter().any(|row| {
            matches!(
                row.state,
                RowState::Preparing | RowState::Uploading(_) | RowState::Scanning
            )
        })
    }

    /// The releases a file can go into (the "Add to an existing album"
    /// list): those TIDAL lists, without deleted ones.
    pub fn existing(&self) -> Vec<(String, String, u32)> {
        self.albums
            .iter()
            .filter(|album| !self.deleted.contains(&album.id))
            .map(|album| {
                let title = self
                    .edits
                    .get(&album.id)
                    .and_then(|edit| edit.title.clone())
                    .unwrap_or_else(|| album.title.clone());
                (album.id.clone(), title, album.tracks)
            })
            .collect()
    }
}

/// A track row of a release's dialog.
#[derive(Clone, Debug, PartialEq)]
pub struct Row {
    pub track_id: Option<String>,
    pub title: String,
    pub access: Access,
    pub duration_secs: Option<u32>,
    pub state: RowState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowState {
    Preparing,
    Uploading(u8),
    /// Processed; moderation is still scanning it.
    Scanning,
    Ready,
    Flagged,
    Failed,
}

fn apply(album: &mut UploadAlbum, change: &AlbumChange) {
    if let Some(title) = &change.title {
        album.title.clone_from(title);
    }
    if let Some(album_type) = change.album_type {
        album.album_type = album_type;
    }
    if let Some(access) = change.access {
        album.access = access;
    }
}

fn agrees(album: &UploadAlbum, change: &AlbumChange) -> bool {
    change
        .title
        .as_ref()
        .is_none_or(|title| &album.title == title)
        && change
            .album_type
            .is_none_or(|kind| album.album_type == kind)
        && change.access.is_none_or(|access| album.access == access)
}

/// Merges a newer change into an older one.
pub fn merge(into: &mut AlbumChange, change: &AlbumChange) {
    if change.title.is_some() {
        into.title.clone_from(&change.title);
    }
    if change.album_type.is_some() {
        into.album_type = change.album_type;
    }
    if change.access.is_some() {
        into.access = change.access;
    }
}

// ---- dialogs -----------------------------------------------------------------

/// The upload dialogs, drawn by `ui::uploads`.
#[derive(Clone, Debug)]
pub enum UploadDialog {
    /// "Organize N new tracks": into an existing release, or a new single.
    Organize {
        files: Vec<PathBuf>,
    },
    /// The radio list of the account's releases and Add.
    AddToExisting {
        files: Vec<PathBuf>,
        selected: Option<String>,
    },
    /// The same list for a track that moves.
    MoveTrack {
        track_id: String,
        from: AlbumKey,
        selected: Option<String>,
        back: Box<EditDraft>,
    },
    /// "New single" / an existing release's details and tracks.
    Edit(Box<EditDraft>),
    /// "Edit name": the artist name uploads are released under.
    EditName {
        artist_id: String,
        name: String,
        back: Box<EditDraft>,
    },
    /// A track's title.
    TrackDetails {
        track_id: String,
        title: String,
        back: Box<EditDraft>,
    },
    ConfirmDeleteAlbum {
        back: Box<EditDraft>,
    },
    ConfirmDeleteTrack {
        track_id: String,
        title: String,
        back: Box<EditDraft>,
    },
}

/// The release dialog's fields while it is open.
#[derive(Clone, Debug)]
pub struct EditDraft {
    pub album: AlbumKey,
    pub is_new: bool,
    pub title: String,
    pub album_type: AlbumType,
    pub access: Access,
    /// What the fields held when it opened: Done sends only what changed.
    pub original: (String, AlbumType, Access),
    /// A cover picked from disk, sent on Done.
    pub new_cover: Option<PathBuf>,
    /// A suggested (spectrogram) cover picked, sent on Done.
    pub suggested_pick: Option<SuggestedCover>,
    /// The spectrograms TIDAL suggested, once asked for.
    pub suggestions: Option<Vec<SuggestedCover>>,
    pub asking_suggestions: bool,
    /// The picked file's bytes, for its preview.
    pub cover_preview: Option<std::sync::Arc<[u8]>>,
}

impl EditDraft {
    pub fn new(album: AlbumKey, details: &UploadAlbum, is_new: bool) -> Self {
        Self {
            album,
            is_new,
            title: details.title.clone(),
            album_type: details.album_type,
            access: details.access,
            original: (details.title.clone(), details.album_type, details.access),
            new_cover: None,
            suggested_pick: None,
            suggestions: None,
            asking_suggestions: false,
            cover_preview: None,
        }
    }

    /// What Done sends.
    pub fn change(&self) -> AlbumChange {
        let title = self.title.trim();
        AlbumChange {
            title: (!title.is_empty() && title != self.original.0).then(|| title.to_string()),
            album_type: (self.album_type != self.original.1).then_some(self.album_type),
            access: (self.access != self.original.2).then_some(self.access),
        }
    }
}

/// What the upload dialogs and the page ask the app to do.
#[derive(Clone, Debug)]
pub enum UploadAction {
    /// "+ Upload": the system's file dialog for audio files.
    Pick,
    /// Files chosen (or dropped).
    Chosen(Vec<PathBuf>),
    /// Organize > Upload as Single: a new release with these files.
    AsSingle(Vec<PathBuf>),
    /// Organize > Add to an existing album.
    ChooseExisting(Vec<PathBuf>),
    /// Add: the files into that release.
    AddTo {
        files: Vec<PathBuf>,
        album_id: String,
    },
    /// A card: its release's dialog.
    OpenAlbum(AlbumKey),
    /// "Add tracks" in a release's dialog.
    AddTracks(AlbumKey),
    /// Done: the draft's changes.
    Save(Box<EditDraft>),
    /// "Change Image".
    PickCover(Box<EditDraft>),
    /// "Create spectrogram".
    Suggest(Box<EditDraft>),
    RenameArtist {
        artist_id: String,
        name: String,
        back: Box<EditDraft>,
    },
    RenameTrack {
        track_id: String,
        title: String,
        back: Box<EditDraft>,
    },
    MoveTrack {
        track_id: String,
        from: AlbumKey,
        to: String,
        back: Box<EditDraft>,
    },
    Reorder {
        album_id: String,
        track_id: String,
        position: u32,
    },
    DeleteTrack {
        track_id: String,
        back: Box<EditDraft>,
    },
    DeleteAlbum(AlbumKey),
    /// A card's menu: the confirmation before deleting that release.
    ConfirmDeleteAlbum(AlbumKey),
    /// A card's menu: `https://tidal.com/upload/<release id>` to the clipboard.
    CopyAlbumLink(AlbumKey),
    CopyTrackLink(String),
    /// Back to a release's dialog from one of its sub-dialogs.
    Back(Box<EditDraft>),
    /// Try the list again.
    Reload,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn album(id: &str, title: &str, tracks: u32) -> UploadAlbum {
        UploadAlbum {
            id: id.into(),
            title: title.into(),
            tracks,
            artist_name: "Me".into(),
            ..Default::default()
        }
    }

    fn file(uploads: &mut Uploads, job: u64, album: AlbumKey, progress: FileProgress) {
        uploads.files.push(PendingFile {
            job,
            index: 0,
            album,
            title: "song".into(),
            path: "song.wav".into(),
            progress,
        });
    }

    #[test]
    fn files_tidal_takes_have_a_content_type() {
        assert_eq!(content_type(Path::new("a.FLAC")), Some("audio/flac"));
        assert_eq!(content_type(Path::new("a.wav")), Some("audio/wav"));
        assert_eq!(content_type(Path::new("a.m4a")), Some("audio/mp4"));
        assert_eq!(content_type(Path::new("a.txt")), None);
        assert_eq!(content_type(Path::new("noext")), None);
        assert_eq!(title_from_path(Path::new("C:/x/My Song.flac")), "My Song");
        assert_eq!(title_from_path(Path::new(".flac")), ".flac");
    }

    #[test]
    fn a_new_release_shows_uploading_before_tidal_lists_it_and_stays_until_it_does() {
        let mut uploads = Uploads::default();
        uploads.absorb(true, vec![album("1", "Old", 2)]);
        let job = uploads.new_job();
        uploads.pending.push(PendingAlbum {
            job,
            id: None,
            title: "Fresh".into(),
            album_type: AlbumType::Single,
            artist_name: "Me".into(),
            files: 1,
            wanted: AlbumChange::default(),
            cover: None,
            delete_when_made: false,
            failed: false,
        });
        file(
            &mut uploads,
            job,
            AlbumKey::Job(job),
            FileProgress::Preparing,
        );
        let cards = uploads.cards();
        assert_eq!(cards.len(), 2);
        assert_eq!(cards[0].title, "Fresh");
        assert!(cards[0].uploading);
        assert!(!cards[1].uploading);
        // TIDAL answers with the id, then a list that lags behind.
        uploads.pending[0].id = Some("2".into());
        uploads.files[0].progress = FileProgress::Sent {
            track_id: "t".into(),
        };
        uploads.absorb(true, vec![album("1", "Old", 2)]);
        let cards = uploads.cards();
        assert_eq!(cards.len(), 2, "the lagging list must not drop the card");
        assert!(!cards[0].uploading);
        // Once listed, TIDAL's own entry takes over, shown once.
        uploads.absorb(true, vec![album("2", "Fresh", 1), album("1", "Old", 2)]);
        assert!(uploads.pending.is_empty());
        assert_eq!(uploads.cards().len(), 2);
    }

    #[test]
    fn deletes_and_edits_hold_until_tidal_agrees() {
        let mut uploads = Uploads::default();
        uploads.absorb(true, vec![album("1", "A", 1), album("2", "B", 1)]);
        uploads.deleted.insert("1".into());
        uploads.edits.insert(
            "2".into(),
            AlbumChange {
                title: Some("B2".into()),
                ..Default::default()
            },
        );
        // A stale answer still has both as they were.
        uploads.absorb(true, vec![album("1", "A", 1), album("2", "B", 1)]);
        let cards = uploads.cards();
        assert_eq!(cards.len(), 1);
        assert_eq!(cards[0].title, "B2");
        // TIDAL catches up.
        uploads.absorb(true, vec![album("2", "B2", 1)]);
        assert!(uploads.deleted.is_empty());
        assert!(uploads.edits.is_empty());
        assert_eq!(uploads.cards()[0].title, "B2");
    }

    #[test]
    fn added_files_show_in_their_release_until_tidal_lists_them() {
        let mut uploads = Uploads::default();
        uploads.absorb(true, vec![album("1", "A", 1)]);
        uploads.tracks.insert(
            "1".into(),
            TrackList {
                tracks: vec![UploadTrack {
                    id: "t1".into(),
                    title: "One".into(),
                    duration_secs: Some(60),
                    file: FileState::Ready,
                    number: 1,
                    ..Default::default()
                }],
                ..Default::default()
            },
        );
        let key = AlbumKey::Id("1".into());
        file(&mut uploads, 5, key.clone(), FileProgress::Uploading(40));
        let rows = uploads.rows(&key);
        assert_eq!(rows.len(), 2);
        assert_eq!(rows[0].state, RowState::Ready);
        assert_eq!(rows[1].state, RowState::Uploading(40));
        assert!(uploads.scanning(&key));
        assert!(uploads.cards()[0].uploading);
        assert_eq!(uploads.cards()[0].tracks, 2);
        uploads.files[0].progress = FileProgress::Sent {
            track_id: "t2".into(),
        };
        assert_eq!(uploads.rows(&key)[1].state, RowState::Preparing);
        uploads.gone_tracks.insert("t1".into());
        assert_eq!(uploads.rows(&key).len(), 1);
    }

    /// A release's dialog opened while it was being made still names it
    /// after TIDAL lists it (its Done once found nothing to send).
    #[test]
    fn a_release_made_here_keeps_its_id_after_tidal_lists_it() {
        let mut uploads = Uploads::default();
        let job = uploads.new_job();
        uploads.pending.push(PendingAlbum {
            job,
            id: Some("9".into()),
            title: "T".into(),
            album_type: AlbumType::Single,
            artist_name: "Me".into(),
            files: 1,
            wanted: AlbumChange::default(),
            cover: None,
            delete_when_made: false,
            failed: false,
        });
        uploads.made.insert(job, "9".into());
        uploads.absorb(true, vec![album("9", "T", 1)]);
        assert!(uploads.pending.is_empty());
        assert_eq!(uploads.album_id(&AlbumKey::Job(job)).as_deref(), Some("9"));
        assert_eq!(uploads.album(&AlbumKey::Job(job)).unwrap().id, "9");
    }

    /// A track moved away (or deleted) here never comes back to its old
    /// release as a "Preparing" row once TIDAL stops listing it there
    /// (seen live: the file sent into the release was still a row of it).
    #[test]
    fn a_moved_track_does_not_return_as_a_preparing_row() {
        let mut uploads = Uploads::default();
        uploads.absorb(true, vec![album("1", "A", 1)]);
        let key = AlbumKey::Id("1".into());
        file(
            &mut uploads,
            4,
            key.clone(),
            FileProgress::Sent {
                track_id: "t9".into(),
            },
        );
        uploads.tracks.insert(
            "1".into(),
            TrackList {
                tracks: vec![UploadTrack {
                    id: "t9".into(),
                    title: "song".into(),
                    duration_secs: Some(4),
                    file: FileState::Ready,
                    ..Default::default()
                }],
                ..Default::default()
            },
        );
        uploads.forget_track("t9");
        // TIDAL's next read no longer has it there.
        uploads.tracks.get_mut("1").unwrap().tracks.clear();
        uploads.gone_tracks.clear();
        assert!(uploads.rows(&key).is_empty());
        assert!(!uploads.scanning(&key));
    }

    #[test]
    fn done_sends_only_what_changed() {
        let details = album("1", "Song", 1);
        let mut draft = EditDraft::new(AlbumKey::Id("1".into()), &details, false);
        assert!(draft.change().is_empty());
        draft.title = "  ".into();
        assert!(draft.change().is_empty(), "an empty title is not sent");
        draft.title = "Song 2".into();
        draft.album_type = AlbumType::Ep;
        let change = draft.change();
        assert_eq!(change.title.as_deref(), Some("Song 2"));
        assert_eq!(change.album_type, Some(AlbumType::Ep));
        assert_eq!(change.access, None);
    }

    #[test]
    fn a_draft_for_a_release_without_an_id_keeps_its_wishes_on_the_card() {
        let mut uploads = Uploads::default();
        uploads.pending.push(PendingAlbum {
            job: 3,
            id: None,
            title: "file".into(),
            album_type: AlbumType::Single,
            artist_name: "Me".into(),
            files: 1,
            wanted: AlbumChange {
                title: Some("Named".into()),
                album_type: Some(AlbumType::Ep),
                access: None,
            },
            cover: None,
            delete_when_made: false,
            failed: false,
        });
        let key = AlbumKey::Job(3);
        assert_eq!(uploads.cards()[0].title, "Named");
        let shown = uploads.album(&key).unwrap();
        assert_eq!(shown.album_type, AlbumType::Ep);
        let mut into = AlbumChange::default();
        merge(
            &mut into,
            &AlbumChange {
                access: Some(Access::Public),
                ..Default::default()
            },
        );
        assert_eq!(into.access, Some(Access::Public));
    }
}

/// Demo builds: a signed-in account's uploads, and the dialogs open in a
/// known state, for screenshots and tests without TIDAL.
#[cfg(any(test, feature = "demo"))]
pub mod demo {
    use super::*;
    use crate::model::Dialog;

    fn release(
        id: &str,
        title: &str,
        kind: AlbumType,
        access: Access,
        tracks: u32,
        seed: Option<u32>,
    ) -> UploadAlbum {
        UploadAlbum {
            id: id.into(),
            title: title.into(),
            album_type: kind,
            access,
            artist_id: Some(crate::demo::UPLOADER.into()),
            artist_name: "Carmine".into(),
            cover: seed.map(|seed| format!("https://picsum.photos/seed/tidalfast{seed}/640/640")),
            tracks,
            explicit: false,
        }
    }

    fn song(id: &str, title: &str, secs: Option<u32>, file: FileState, number: u32) -> UploadTrack {
        UploadTrack {
            id: id.into(),
            title: title.into(),
            access: Access::Unlisted,
            duration_secs: secs,
            file,
            number,
        }
    }

    /// The account's releases: public and unlisted, with and without a
    /// cover, and a single being made with its file on the way.
    pub fn populate(uploads: &mut Uploads) {
        uploads.artist = Some(Some(UploadArtist {
            id: crate::demo::UPLOADER.into(),
            name: "Carmine".into(),
        }));
        uploads.absorb(
            true,
            vec![
                release(
                    "up1",
                    "Night Drive Sketches",
                    AlbumType::Ep,
                    Access::Unlisted,
                    4,
                    Some(31),
                ),
                release(
                    "up2",
                    "Harbour Lights",
                    AlbumType::Single,
                    Access::Public,
                    1,
                    Some(32),
                ),
                release(
                    "up3",
                    "Untitled",
                    AlbumType::Single,
                    Access::Unlisted,
                    1,
                    None,
                ),
                release(
                    "up4",
                    "Field Recordings Vol. 1",
                    AlbumType::Album,
                    Access::Public,
                    9,
                    Some(33),
                ),
                release(
                    "up5",
                    "Demo 2026",
                    AlbumType::Single,
                    Access::Unlisted,
                    2,
                    None,
                ),
            ],
        );
        uploads.tracks.insert(
            "up1".into(),
            TrackList {
                tracks: vec![
                    song("t11", "Night Drive", Some(212), FileState::Ready, 1),
                    song("t12", "Overpass", Some(187), FileState::Ready, 2),
                    song("t13", "Sodium Lamps", Some(245), FileState::Ready, 3),
                    song("t14", "Last Exit", None, FileState::Preparing, 4),
                ],
                ..Default::default()
            },
        );
        uploads.tracks.insert(
            "up2".into(),
            TrackList {
                tracks: vec![song(
                    "t21",
                    "Harbour Lights",
                    Some(198),
                    FileState::Ready,
                    1,
                )],
                ..Default::default()
            },
        );
        let job = uploads.new_job();
        uploads.pending.push(PendingAlbum {
            job,
            id: None,
            title: "Morning Take".into(),
            album_type: AlbumType::Single,
            artist_name: "Carmine".into(),
            files: 1,
            wanted: AlbumChange::default(),
            cover: None,
            delete_when_made: false,
            failed: false,
        });
        uploads.files.push(PendingFile {
            job,
            index: 0,
            album: AlbumKey::Job(job),
            title: "Morning Take".into(),
            path: "Morning Take.wav".into(),
            progress: FileProgress::Preparing,
        });
    }

    fn draft(app: &crate::app::App, key: AlbumKey, is_new: bool) -> Box<EditDraft> {
        let details = app.uploads.album(&key).unwrap_or_default();
        Box::new(EditDraft::new(key, &details, is_new))
    }

    /// Opens a dialog for `--demo-show`; false for a name it does not know.
    pub fn surface(app: &mut crate::app::App, name: &str) -> bool {
        let file = || vec![std::path::PathBuf::from("Morning Take.wav")];
        let dialog = match name {
            "upload-organize" => UploadDialog::Organize { files: file() },
            "upload-existing" => UploadDialog::AddToExisting {
                files: file(),
                selected: Some("up1".into()),
            },
            // The single being made: its file still on the way.
            "upload-new-single" => UploadDialog::Edit(draft(app, AlbumKey::Job(1), true)),
            // A new single whose file is in: its dropdowns can open.
            "upload-type-open" | "upload-visibility-open" => {
                app.uploads.demo_popup = Some(egui::Id::new(if name == "upload-type-open" {
                    "upload-album-type"
                } else {
                    "upload-visibility"
                }));
                UploadDialog::Edit(draft(app, AlbumKey::Id("up2".into()), true))
            }
            "upload-edit" => UploadDialog::Edit(draft(app, AlbumKey::Id("up1".into()), false)),
            "upload-track-menu" => {
                app.uploads.demo_popup = Some(egui::Id::new(("upload-track-menu", "t12")));
                UploadDialog::Edit(draft(app, AlbumKey::Id("up1".into()), false))
            }
            "upload-edit-name" => UploadDialog::EditName {
                artist_id: crate::demo::UPLOADER.into(),
                name: "Carmine".into(),
                back: draft(app, AlbumKey::Id("up1".into()), false),
            },
            "upload-delete" => UploadDialog::ConfirmDeleteAlbum {
                back: draft(app, AlbumKey::Id("up1".into()), false),
            },
            _ => return false,
        };
        app.dialog = Some(Dialog::Upload(Box::new(dialog)));
        true
    }
}
