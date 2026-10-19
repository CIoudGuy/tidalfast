//! The backend's side of TIDAL Upload: each request the Upload page makes,
//! run on the backend's runtime, and the file uploads themselves, streamed
//! from disk on a thread of their own with their progress reported as they
//! go. Nothing here runs on the interface's thread.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use md5::Digest;

use crate::api::ApiError;
use crate::api::tidal::TidalClient;
use crate::api::tidal::uploads::{
    Access, AlbumChange, AlbumType, Paged, SuggestedCover, UploadAlbum, UploadArtist, UploadLink,
    UploadTrack,
};
use crate::backend::ApiResult;

/// How many pages of a list are read at most (TIDAL pages by 20 or so).
const MAX_PAGES: usize = 50;

/// What the Upload page asks the backend for.
#[derive(Clone, Debug, PartialEq)]
pub enum UploadRequest {
    /// The account's releases, a page at a time.
    List {
        cursor: Option<String>,
    },
    /// The account's artist.
    Artist,
    /// A release's tracks.
    Tracks {
        album_id: String,
    },
    CreateAlbum {
        job: u64,
        title: String,
        album_type: AlbumType,
        artist_id: String,
    },
    /// One file: its track, its source file and the file itself.
    File {
        job: u64,
        index: usize,
        path: PathBuf,
        album_id: String,
        artist_id: String,
        title: String,
        access: Access,
    },
    Change {
        album_id: String,
        change: AlbumChange,
    },
    /// A cover image from disk.
    Cover {
        album_id: String,
        path: PathBuf,
    },
    /// A suggested cover (a spectrogram).
    SetCover {
        album_id: String,
        artwork_id: String,
    },
    Suggest {
        album_id: String,
    },
    RenameTrack {
        track_id: String,
        title: String,
    },
    MoveTrack {
        track_id: String,
        album_id: String,
    },
    Reorder {
        album_id: String,
        track_id: String,
        position: u32,
    },
    RenameArtist {
        artist_id: String,
        name: String,
    },
    DeleteTrack {
        track_id: String,
    },
    DeleteAlbum {
        album_id: String,
    },
}

impl UploadRequest {
    /// Requests that send a file and report their progress on the way.
    pub fn streams(&self) -> bool {
        matches!(self, Self::File { .. })
    }

    fn written(&self) -> Option<Written> {
        Some(match self {
            Self::Change { album_id, .. }
            | Self::Cover { album_id, .. }
            | Self::SetCover { album_id, .. } => Written::Album {
                album_id: album_id.clone(),
            },
            Self::RenameTrack { track_id, .. }
            | Self::MoveTrack { track_id, .. }
            | Self::DeleteTrack { track_id } => Written::Track {
                track_id: track_id.clone(),
            },
            Self::Reorder { album_id, .. } => Written::Album {
                album_id: album_id.clone(),
            },
            Self::RenameArtist { .. } => Written::Artist,
            Self::DeleteAlbum { album_id } => Written::Deleted {
                album_id: album_id.clone(),
            },
            _ => return None,
        })
    }
}

/// What a write was about, for its answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Written {
    Album { album_id: String },
    Track { track_id: String },
    Artist,
    Deleted { album_id: String },
}

#[derive(Clone, Debug)]
pub enum UploadResponse {
    List {
        first: bool,
        result: ApiResult<Paged<UploadAlbum>>,
    },
    Artist(ApiResult<Option<UploadArtist>>),
    Tracks {
        album_id: String,
        result: ApiResult<Vec<UploadTrack>>,
    },
    AlbumCreated {
        job: u64,
        result: ApiResult<String>,
    },
    Progress {
        job: u64,
        index: usize,
        percent: u8,
    },
    /// A file was sent (its track id), or failed.
    FileDone {
        job: u64,
        index: usize,
        result: ApiResult<String>,
    },
    Suggested {
        album_id: String,
        result: ApiResult<Vec<SuggestedCover>>,
    },
    Written {
        what: Written,
        request: Box<UploadRequest>,
        result: ApiResult<()>,
    },
}

impl UploadResponse {
    /// The answer a request gets when there is no session to run it with.
    pub fn failed(request: UploadRequest, error: ApiError) -> Self {
        match request {
            UploadRequest::List { cursor } => Self::List {
                first: cursor.is_none(),
                result: Err(error),
            },
            UploadRequest::Artist => Self::Artist(Err(error)),
            UploadRequest::Tracks { album_id } => Self::Tracks {
                album_id,
                result: Err(error),
            },
            UploadRequest::CreateAlbum { job, .. } => Self::AlbumCreated {
                job,
                result: Err(error),
            },
            UploadRequest::File { job, index, .. } => Self::FileDone {
                job,
                index,
                result: Err(error),
            },
            UploadRequest::Suggest { album_id } => Self::Suggested {
                album_id,
                result: Err(error),
            },
            other => Self::Written {
                what: other.written().unwrap_or(Written::Artist),
                request: Box::new(other),
                result: Err(error),
            },
        }
    }
}

/// Runs one request that sends no file.
pub async fn handle(client: &TidalClient, request: UploadRequest) -> UploadResponse {
    match request {
        UploadRequest::List { cursor } => UploadResponse::List {
            first: cursor.is_none(),
            result: client.upload_albums(cursor.as_deref()).await,
        },
        UploadRequest::Artist => UploadResponse::Artist(client.upload_artist().await),
        UploadRequest::Tracks { album_id } => {
            let result = all_tracks(client, &album_id).await;
            UploadResponse::Tracks { album_id, result }
        }
        UploadRequest::CreateAlbum {
            job,
            title,
            album_type,
            artist_id,
        } => UploadResponse::AlbumCreated {
            job,
            result: client
                .create_upload_album(&title, album_type, &artist_id)
                .await,
        },
        UploadRequest::Suggest { album_id } => UploadResponse::Suggested {
            result: client.suggested_covers(&album_id).await,
            album_id,
        },
        UploadRequest::File { job, index, .. } => UploadResponse::FileDone {
            job,
            index,
            result: Err(ApiError::Decode(
                "a file goes through the upload thread".into(),
            )),
        },
        write => {
            let what = write.written().unwrap_or(Written::Artist);
            let result = run_write(client, &write).await;
            UploadResponse::Written {
                what,
                request: Box::new(write),
                result,
            }
        }
    }
}

async fn all_tracks(client: &TidalClient, album_id: &str) -> ApiResult<Vec<UploadTrack>> {
    let mut tracks = Vec::new();
    let mut cursor: Option<String> = None;
    for _ in 0..MAX_PAGES {
        let page = client.upload_tracks(album_id, cursor.as_deref()).await?;
        let empty = page.items.is_empty();
        tracks.extend(page.items);
        match page.next {
            Some(next) if !empty && Some(&next) != cursor.as_ref() => cursor = Some(next),
            _ => break,
        }
    }
    Ok(tracks)
}

async fn run_write(client: &TidalClient, request: &UploadRequest) -> ApiResult<()> {
    match request {
        UploadRequest::Change { album_id, change } => {
            client.change_upload_album(album_id, change).await
        }
        UploadRequest::Cover { album_id, path } => {
            let path = path.clone();
            let (bytes, md5_hex) = tokio::task::spawn_blocking(move || read_image(&path))
                .await
                .map_err(|_| ApiError::Decode("the image could not be read".into()))??;
            let (artwork_id, link) = client.create_artwork(&md5_hex, bytes.len() as u64).await?;
            let content_type = if bytes.starts_with(&[0x89, b'P', b'N', b'G']) {
                "image/png"
            } else {
                "image/jpeg"
            };
            client.put_upload(&link, bytes, content_type).await?;
            client.set_upload_cover(album_id, &artwork_id).await
        }
        UploadRequest::SetCover {
            album_id,
            artwork_id,
        } => client.set_upload_cover(album_id, artwork_id).await,
        UploadRequest::RenameTrack { track_id, title } => {
            client.rename_upload_track(track_id, title).await
        }
        UploadRequest::MoveTrack { track_id, album_id } => {
            client.move_upload_track(track_id, album_id).await
        }
        UploadRequest::Reorder {
            album_id,
            track_id,
            position,
        } => {
            client
                .reorder_upload_track(album_id, track_id, *position)
                .await
        }
        UploadRequest::RenameArtist { artist_id, name } => {
            client.rename_upload_artist(artist_id, name).await
        }
        UploadRequest::DeleteTrack { track_id } => client.delete_upload_track(track_id).await,
        UploadRequest::DeleteAlbum { album_id } => client.delete_upload_album(album_id).await,
        _ => Ok(()),
    }
}

/// A cover image: at most 20 MB, JPEG or PNG.
fn read_image(path: &Path) -> ApiResult<(Vec<u8>, String)> {
    let bytes =
        std::fs::read(path).map_err(|_| ApiError::Decode("Couldn't open that image.".into()))?;
    let jpeg = bytes.starts_with(&[0xff, 0xd8]);
    let png = bytes.starts_with(&[0x89, b'P', b'N', b'G']);
    if bytes.len() > 20 * 1024 * 1024 || !(jpeg || png) {
        return Err(ApiError::Decode("Choose a JPEG or PNG image.".into()));
    }
    let md5_hex = hex(&md5::Md5::digest(&bytes));
    Ok((bytes, md5_hex))
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

/// A file's size and lowercase hex MD5, read in 1 MB pieces.
pub fn digest_file(path: &Path) -> std::io::Result<(u64, String)> {
    let mut file = std::fs::File::open(path)?;
    let mut hasher = md5::Md5::new();
    let mut buffer = vec![0_u8; 1 << 20];
    let mut size = 0_u64;
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        size += read as u64;
        hasher.update(&buffer[..read]);
    }
    Ok((size, hex(&hasher.finalize())))
}

/// Why a chosen file cannot go, before anything is asked of TIDAL.
pub fn check_file(path: &Path, size: u64) -> Result<&'static str, String> {
    let Some(content_type) = crate::uploads::content_type(path).filter(|t| t.starts_with("audio/"))
    else {
        return Err("This file format isn't supported.".into());
    };
    if size == 0 {
        return Err("This file is empty.".into());
    }
    if size > crate::uploads::MAX_FILE {
        return Err("This file is larger than 5 GB.".into());
    }
    Ok(content_type)
}

/// A reader that reports how much of the file has gone, at each whole
/// percent (never more often).
struct Counting<R, F> {
    inner: R,
    sent: u64,
    total: u64,
    percent: u8,
    report: F,
}

impl<R: Read, F: FnMut(u8)> Read for Counting<R, F> {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let read = self.inner.read(buf)?;
        self.sent += read as u64;
        let percent = (self.sent.saturating_mul(100) / self.total.max(1)).min(100) as u8;
        if percent > self.percent {
            self.percent = percent;
            (self.report)(percent);
        }
        Ok(read)
    }
}

/// Sends a file to its upload address, streaming it from disk.
pub fn send_file(
    proxy: &crate::settings::ProxyConfig,
    link: &UploadLink,
    path: &Path,
    size: u64,
    content_type: &str,
    report: impl FnMut(u8) + Send + 'static,
) -> Result<(), String> {
    let client = crate::http::blocking_builder(proxy)?
        .connect_timeout(Duration::from_secs(15))
        .timeout(None)
        .build()
        .map_err(|error| error.without_url().to_string())?;
    let file = std::fs::File::open(path).map_err(|_| "The file could not be read.".to_string())?;
    let body = reqwest::blocking::Body::sized(
        Counting {
            inner: file,
            sent: 0,
            total: size,
            percent: 0,
            report,
        },
        size,
    );
    let method = reqwest::Method::from_bytes(link.method.as_bytes())
        .map_err(|_| "TIDAL asked for an unknown upload method".to_string())?;
    let mut request = client
        .request(method, &link.href)
        .header(reqwest::header::CONTENT_TYPE, content_type)
        .body(body);
    for (key, value) in &link.headers {
        request = request.header(key.as_str(), value.as_str());
    }
    let response = request
        .send()
        .map_err(|error| format!("Upload failed: {}", error.without_url()))?;
    let status = response.status();
    // The address is signed: never logged.
    log::debug!("TIDAL upload PUT answered {}", status.as_u16());
    if status.is_success() {
        Ok(())
    } else {
        Err(format!("Upload failed ({})", status.as_u16()))
    }
}

/// One file, start to end: checks it, makes its track, announces its source
/// file and sends it. A failure after the track exists deletes the track
/// again (TIDAL's web player does the same).
pub async fn run_file(
    client: Arc<TidalClient>,
    proxy: crate::settings::ProxyConfig,
    request: UploadRequest,
    progress: impl Fn(u8) + Send + Sync + 'static,
) -> ApiResult<String> {
    let UploadRequest::File {
        path,
        album_id,
        artist_id,
        title,
        access,
        ..
    } = request
    else {
        return Err(ApiError::Decode("not a file".into()));
    };
    let hashed = path.clone();
    let (size, md5_hex) = tokio::task::spawn_blocking(move || digest_file(&hashed))
        .await
        .map_err(|_| ApiError::Decode("the file could not be read".into()))?
        .map_err(|_| ApiError::Decode("The file could not be read.".into()))?;
    let content_type = check_file(&path, size).map_err(ApiError::Decode)?;
    let track_id = client
        .create_upload_track(&title, access, &album_id, &artist_id)
        .await?;
    let sent = async {
        let link = client
            .create_track_source(&track_id, &md5_hex, size)
            .await?;
        let (done, wait) = tokio::sync::oneshot::channel();
        let progress = Arc::new(progress);
        std::thread::Builder::new()
            .name("upload".into())
            .spawn(move || {
                let report = move |percent| progress(percent);
                let _ = done.send(send_file(&proxy, &link, &path, size, content_type, report));
            })
            .map_err(|error| ApiError::Network(error.to_string()))?;
        wait.await
            .map_err(|_| ApiError::Network("the upload stopped".into()))?
            .map_err(ApiError::Network)
    }
    .await;
    match sent {
        Ok(()) => Ok(track_id),
        Err(error) => {
            if let Err(cleanup) = client.delete_upload_track(&track_id).await {
                log::warn!("unable to remove the track of a failed upload: {cleanup}");
            }
            Err(error)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_digest_is_tidals_lowercase_hex_md5() {
        let dir = std::env::temp_dir().join(format!("tidalfast-upload-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("a.wav");
        std::fs::write(&path, b"abc").unwrap();
        assert_eq!(
            digest_file(&path).unwrap(),
            (3, "900150983cd24fb0d6963f7d28e17f72".to_string())
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn files_are_checked_before_anything_is_asked() {
        assert_eq!(check_file(Path::new("a.flac"), 10), Ok("audio/flac"));
        assert!(check_file(Path::new("a.txt"), 10).is_err());
        assert!(
            check_file(Path::new("a.png"), 10).is_err(),
            "covers are not tracks"
        );
        assert!(check_file(Path::new("a.wav"), 0).is_err());
        assert!(check_file(Path::new("a.wav"), crate::uploads::MAX_FILE + 1).is_err());
    }

    #[test]
    fn progress_is_reported_once_per_percent() {
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let log = seen.clone();
        let mut reader = Counting {
            inner: std::io::Cursor::new(vec![0_u8; 1000]),
            sent: 0,
            total: 1000,
            percent: 0,
            report: move |p| log.lock().unwrap().push(p),
        };
        let mut buffer = [0_u8; 3];
        while reader.read(&mut buffer).unwrap() > 0 {}
        let seen = seen.lock().unwrap();
        assert_eq!(seen.first(), Some(&1));
        assert_eq!(seen.last(), Some(&100));
        assert_eq!(seen.len(), 100);
    }

    #[test]
    fn a_request_without_a_session_answers_its_own_kind() {
        let error = ApiError::NotSignedIn;
        assert!(matches!(
            UploadResponse::failed(
                UploadRequest::DeleteAlbum {
                    album_id: "1".into()
                },
                error.clone()
            ),
            UploadResponse::Written {
                what: Written::Deleted { .. },
                ..
            }
        ));
        assert!(matches!(
            UploadResponse::failed(UploadRequest::List { cursor: None }, error),
            UploadResponse::List { first: true, .. }
        ));
    }
}
