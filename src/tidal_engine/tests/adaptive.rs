//! Adaptive streaming in the engine: what the downloads measure, and the
//! quality the next song is asked at.

use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::sync::atomic::AtomicBool;

use super::*;
use crate::tidal_engine::buffer::Meter;

/// Serves `body` at every path, `pace` bytes every 10 ms; `sent` turns
/// true once a response has been written whole.
fn slow_server(body: Vec<u8>, pace: usize) -> (String, Arc<AtomicBool>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/track", listener.local_addr().unwrap());
    let sent = Arc::new(AtomicBool::new(false));
    let done = Arc::clone(&sent);
    let body = Arc::new(body);
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { return };
            let body = Arc::clone(&body);
            let done = Arc::clone(&done);
            thread::spawn(move || {
                let mut reader = BufReader::new(stream.try_clone().unwrap());
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                }
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                if stream.write_all(head.as_bytes()).is_err() {
                    return;
                }
                // Each chunk is due 10 ms after the one before, counted
                // from the start: a sleep that oversleeps (a loaded macOS
                // runner's do) is made up by the next chunk, so the rate
                // stays the one asked for rather than the timer's.
                let start = Instant::now();
                for (sent, chunk) in body.chunks(pace).enumerate() {
                    if stream.write_all(chunk).is_err() {
                        return;
                    }
                    let due = start + Duration::from_millis(10) * (sent as u32 + 1);
                    thread::sleep(due.saturating_duration_since(Instant::now()));
                }
                done.store(true, Ordering::SeqCst);
            });
        }
    });
    (url, sent)
}

#[test]
fn a_download_meter_measures_the_rate_the_bytes_arrive_at() {
    let body = vec![7u8; 200_000];
    // At most 200 kB/s: 1.6 Mbit/s.
    let (url, _) = slow_server(body, 2_000);
    let meter = Arc::new(Meter::default());
    let download = Download::start(Fetch {
        meter: Some(Arc::clone(&meter)),
        ..Fetch::new(client(), vec![url])
    });
    assert_eq!(download.wait(Duration::from_secs(30)), Some(Ok(())));
    let reading = meter.take().expect("a reading");
    assert_eq!(reading.bytes, 200_000);
    assert!(reading.finished);
    assert_eq!(reading.whole, Some(200_000));
    assert!(!meter.running());
    let kbps = reading.bytes * 8 * 1000 / reading.micros;
    assert!(kbps > 0 && kbps <= 1_700, "{kbps} kbit/s");
    // One song counts once.
    assert_eq!(meter.take(), None);
}

/// One album, `tidal:album:1`, of tracks 1 to 4 that are one second each,
/// served slowly; every quality asked is noted.
struct Slow {
    url: String,
    sent: Arc<AtomicBool>,
    asked: Mutex<Vec<(String, Quality)>>,
}

impl Catalog for Slow {
    fn context_tracks(&self, _: &str) -> Result<Vec<LocalTrack>> {
        Ok((1..=4).map(track).collect())
    }

    fn tracks(&self, _: &[String]) -> Result<Vec<LocalTrack>> {
        Ok(Vec::new())
    }

    fn stream(&self, track_id: &str, quality: Quality) -> Result<StreamInfo> {
        lock(&self.asked).push((track_id.to_string(), quality));
        Ok(StreamInfo {
            urls: vec![self.url.clone()],
            mime: "audio/wav".into(),
            codec: "pcm".into(),
            quality,
            ..StreamInfo::default()
        })
    }

    fn autoplay(&self, _: &str) -> Result<Vec<LocalTrack>> {
        Ok(Vec::new())
    }
}

/// Plays track 1 of a stream that arrives at about 800 kbit/s (a second
/// of CD audio is 1.4 Mbit/s), then goes on to track 2. Returns the
/// qualities asked.
fn next_after_a_slow_song(adaptive: bool) -> Vec<(String, Quality)> {
    let (url, sent) = slow_server(wav(decode::RATE, 1.0), 1_000);
    let catalog = Arc::new(Slow {
        url,
        sent,
        asked: Mutex::default(),
    });
    let config = EngineConfig {
        adaptive,
        ..config()
    };
    let (engine, states) = engine_with(config, Arc::clone(&catalog) as Arc<dyn Catalog>);
    until(&states, |state| state.connected);
    load_album(&engine, Some("tidal:track:1"));
    until(&states, paused_on("tidal:track:1"));
    // The whole of track 1 arrives, paused as it is.
    let started = Instant::now();
    while !catalog.sent.load(Ordering::SeqCst) {
        assert!(
            started.elapsed() < Duration::from_secs(30),
            "track 1 never arrived"
        );
        thread::sleep(Duration::from_millis(20));
    }
    thread::sleep(Duration::from_millis(500));
    engine.command(PlayerCommand::Next).unwrap();
    let on_two = until(&states, paused_on("tidal:track:2"));
    // The pill shows what TIDAL gave for the song playing.
    let asked_two = lock(&catalog.asked).last().unwrap().1;
    assert_eq!(
        on_two.format.and_then(|format| format.quality),
        Some(asked_two)
    );
    engine.shutdown();
    lock(&catalog.asked).clone()
}

#[test]
fn a_connection_too_slow_for_the_quality_asks_the_next_song_lower() {
    let asked = next_after_a_slow_song(true);
    assert_eq!(
        asked,
        vec![
            ("1".to_string(), Quality::Lossless),
            ("2".to_string(), Quality::High),
        ]
    );
}

#[test]
fn with_adaptive_streaming_off_every_song_is_asked_at_the_chosen_quality() {
    let asked = next_after_a_slow_song(false);
    assert_eq!(
        asked,
        vec![
            ("1".to_string(), Quality::Lossless),
            ("2".to_string(), Quality::Lossless),
        ]
    );
}
