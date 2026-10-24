//! The Feed row's counter in the sidebar, kept as TIDAL 2.46.0 keeps its
//! `feed.unseenFeedItemsCount`.
//!
//! TIDAL asks how many Feed activities are unseen (`checkFeedUpdates`:
//! `GET v2/feed/activities?userId=&limit=9`, reading the answer's
//! `stats.totalNotSeenActivities`) as soon as its polling starts and every
//! five minutes while it runs. Polling starts at sign-in, when the network
//! comes back and when the window shows again (`visibilitychange`); it
//! stops while the window is hidden (minimised, or closed to the tray) and
//! while the network is gone. The Feed page's own answer sets the count
//! too. Once the Feed page has loaded with unseen activities, TIDAL clears
//! the count and tells the server (`hasSeenFeed`:
//! `PUT v2/feed/activities/seen?userId=`).
//!
//! Tidalfast does the same, optimistically: the count is 0 the moment the
//! Feed has been seen, and an answer to a request sent before TIDAL took the
//! PUT tells the story from before, so its count is dropped. A PUT the
//! network did not carry is owed and sent again before the next check, so
//! the number never comes back for something already seen. Nothing is asked
//! in offline mode or in demo mode.

use super::*;
use crate::api::ApiError;
use crate::backend::ApiResult;

/// TIDAL's polling period (`startPollingData`'s `delay(3e5)`).
pub(crate) const FEED_CHECK_EVERY: Duration = Duration::from_secs(300);

/// Where telling TIDAL the Feed was seen stands.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Seen {
    /// Nothing to tell.
    #[default]
    Told,
    /// The PUT is on its way.
    Telling,
    /// The PUT could not go or did not arrive: it goes again before the
    /// next check.
    Owed,
}

/// What the counter asks the backend for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FeedCall {
    /// The light count check, numbered.
    Check(u64),
    /// The Feed was seen, numbered too.
    TellSeen(u64),
}

#[derive(Debug, Default)]
pub struct FeedBadge {
    /// The number the Feed row shows; 0 shows nothing.
    count: u32,
    /// The next request's number (Feed page loads and count checks share
    /// them).
    next_serial: u64,
    /// Answers to requests numbered below this were asked before TIDAL took
    /// the Feed as seen: their count is stale.
    fresh_from: u64,
    /// Answers to requests numbered below this were the previous account's.
    account_from: u64,
    /// The Feed page's rows have answered since they were last asked for
    /// (TIDAL's `feed.status === "loaded"`).
    loaded: bool,
    /// The count check on its way: a second one waits for its answer.
    checking: Option<u64>,
    /// When the last count check went out.
    checked_at: Option<Instant>,
    /// Polling runs (TIDAL's `session.isPolling`).
    polling: bool,
    /// Polling just started: it asks at once.
    check_due: bool,
    seen: Seen,
}

impl FeedBadge {
    /// The number the Feed row shows.
    pub fn count(&self) -> u32 {
        self.count
    }

    fn serial(&mut self) -> u64 {
        let serial = self.next_serial;
        self.next_serial += 1;
        serial
    }

    /// A count from an answer to request `serial` is today's.
    fn fresh(&self, serial: u64) -> bool {
        serial >= self.fresh_from && serial >= self.account_from && self.seen == Seen::Told
    }

    /// The Feed page asks for its rows: the request's number.
    pub(crate) fn feed_requested(&mut self) -> u64 {
        self.loaded = false;
        self.serial()
    }

    /// The Feed page's rows answered (`count` is `None` for a failure).
    /// `false` for an answer asked for the previous account, which is
    /// dropped whole.
    pub(crate) fn feed_answered(&mut self, serial: u64, count: Option<u32>) -> bool {
        if serial < self.account_from {
            return false;
        }
        if let Some(count) = count {
            self.loaded = true;
            if self.fresh(serial) {
                self.count = count;
            }
        }
        true
    }

    /// The count check `serial` answered (`None` for a failure, which
    /// leaves the count as it was, as TIDAL's `failSilently` does).
    pub(crate) fn unseen_answered(&mut self, serial: u64, count: Option<u32>) {
        if self.checking == Some(serial) {
            self.checking = None;
        }
        if let Some(count) = count.filter(|_| self.fresh(serial)) {
            self.count = count;
        }
    }

    /// One pass of the polling: `can` is whether TIDAL may be asked now
    /// (signed in, online, not offline mode, the window showing). Returns
    /// the request to send, if one is due.
    pub(crate) fn poll(&mut self, now: Instant, can: bool) -> Option<FeedCall> {
        if !can {
            self.polling = false;
            self.check_due = false;
            return None;
        }
        if !self.polling {
            self.polling = true;
            self.check_due = true;
        }
        let due = self.check_due
            || self
                .checked_at
                .is_none_or(|at| now.saturating_duration_since(at) >= FEED_CHECK_EVERY);
        if !due {
            return None;
        }
        match self.seen {
            // The check waits for TIDAL to take the Feed as seen.
            Seen::Telling => return None,
            Seen::Owed => {
                self.seen = Seen::Telling;
                return Some(FeedCall::TellSeen(self.serial()));
            }
            Seen::Told => {}
        }
        self.check_due = false;
        if self.checking.is_some() {
            // The check on its way answers for this one too.
            return None;
        }
        let serial = self.serial();
        self.checking = Some(serial);
        self.checked_at = Some(now);
        Some(FeedCall::Check(serial))
    }

    /// TIDAL's Feed page clears the count once its rows have loaded while
    /// some activity is unseen.
    pub(crate) fn seen_due(&self, on_feed: bool) -> bool {
        on_feed && self.loaded && self.count > 0
    }

    /// The Feed was seen: the count is 0 from now on, and every answer
    /// already asked for is older than that. Returns the PUT's number when
    /// TIDAL is to be told now; when it cannot be (`reachable` false) it is
    /// owed.
    pub(crate) fn mark_seen(&mut self, reachable: bool) -> Option<u64> {
        self.count = 0;
        self.fresh_from = self.next_serial;
        if reachable {
            self.seen = Seen::Telling;
            Some(self.serial())
        } else {
            self.seen = Seen::Owed;
            None
        }
    }

    /// TIDAL answered the PUT `serial`: `Ok`, or whether it should go again
    /// (no network, a server failure, a rate limit) rather than be given up
    /// (a refusal, which TIDAL's `allow40X` lets pass). The answer to the
    /// last account's PUT is dropped: the next one's Feed is not owed.
    pub(crate) fn seen_answered(&mut self, serial: u64, result: Result<(), bool>) {
        if serial < self.account_from {
            return;
        }
        // What was asked while the PUT was on its way is from before.
        self.fresh_from = self.next_serial;
        self.seen = match result {
            Err(true) => Seen::Owed,
            Ok(()) | Err(false) => Seen::Told,
        };
    }

    /// Another account (or none) is signed in: nothing of the last one's
    /// counts, and its answers still on their way are dropped.
    pub(crate) fn reset(&mut self) {
        let next_serial = self.next_serial;
        *self = Self {
            next_serial,
            fresh_from: next_serial,
            account_from: next_serial,
            ..Self::default()
        };
    }
}

/// Whether a failed PUT should go again: no answer, a rate limit, or a
/// server failure, not a refusal.
fn tell_again(error: &ApiError) -> bool {
    match error {
        ApiError::Network(_) | ApiError::RateLimited | ApiError::QuotaExhausted => true,
        ApiError::Status { status, .. } => *status == 429 || *status >= 500,
        _ => false,
    }
}

impl App {
    /// The number on the sidebar's Feed row (0 shows none).
    pub fn feed_unseen(&self) -> u32 {
        self.feed_badge.count()
    }

    /// TIDAL may be asked about the Feed now.
    fn feed_reachable(&self) -> bool {
        self.is_connected() && !self.offline && self.network_reachable
    }

    /// Asks for the Feed page's rows, keeping the rows already shown until
    /// the answer replaces them.
    pub(super) fn request_feed(&mut self) {
        // The demo's sample Feed is all there is: nothing would answer.
        if self.offline {
            return;
        }
        if !matches!(self.feed, Loadable::Loaded(_)) {
            self.feed = Loadable::Loading;
        }
        let serial = self.feed_badge.feed_requested();
        self.backend.api(ApiRequest::Feed { serial });
    }

    /// TIDAL's polling of the Feed counter, run with the logic every pass;
    /// `shown` is whether the window shows (not hidden, not minimised).
    pub(super) fn poll_feed_badge(&mut self, shown: bool) {
        let can = shown && self.feed_reachable();
        match self.feed_badge.poll(Instant::now(), can) {
            Some(FeedCall::Check(serial)) => {
                self.backend.api(ApiRequest::FeedUnseen { serial });
            }
            Some(FeedCall::TellSeen(serial)) => self.backend.api(ApiRequest::FeedSeen { serial }),
            None => {}
        }
        self.sync_feed_seen();
    }

    /// On the Feed page, once its rows have loaded with unseen activities,
    /// they are seen: the counter goes at once and TIDAL is told.
    pub(super) fn sync_feed_seen(&mut self) {
        if !self.feed_badge.seen_due(matches!(self.page(), Page::Feed)) {
            return;
        }
        if let Some(serial) = self.feed_badge.mark_seen(self.feed_reachable()) {
            self.backend.api(ApiRequest::FeedSeen { serial });
        }
    }

    /// The Feed page's rows answered.
    pub(super) fn handle_feed(&mut self, serial: u64, result: ApiResult<crate::api::models::Feed>) {
        let count = result.as_ref().ok().map(|feed| feed.unseen);
        if !self.feed_badge.feed_answered(serial, count) {
            return;
        }
        self.feed = match result {
            Ok(feed) => Loadable::Loaded(feed),
            // A failed refresh keeps the rows already shown.
            Err(_) if matches!(self.feed, Loadable::Loaded(_)) => return,
            Err(error) => Loadable::Failed(friendly_api_error(self.locale, &error)),
        };
        self.sync_feed_seen();
    }

    /// The count check answered.
    pub(super) fn handle_feed_unseen(&mut self, serial: u64, result: ApiResult<u32>) {
        self.feed_badge.unseen_answered(serial, result.ok());
        self.sync_feed_seen();
    }

    /// TIDAL answered that the Feed was seen.
    pub(super) fn handle_feed_seen(&mut self, serial: u64, result: ApiResult<()>) {
        if let Err(error) = &result {
            log::debug!("TIDAL feed: marking seen failed: {error}");
        }
        self.feed_badge
            .seen_answered(serial, result.map_err(|error| tell_again(&error)));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn polling(badge: &mut FeedBadge, now: Instant) -> u64 {
        match badge.poll(now, true) {
            Some(FeedCall::Check(serial)) => serial,
            other => panic!("expected a check, got {other:?}"),
        }
    }

    #[test]
    fn polling_asks_at_once_then_every_five_minutes_and_shares_the_request_on_its_way() {
        let start = Instant::now();
        let mut badge = FeedBadge::default();
        let first = polling(&mut badge, start);
        // On its way: no second request, however often the logic runs.
        assert_eq!(badge.poll(start + Duration::from_secs(1), true), None);
        badge.unseen_answered(first, Some(1));
        assert_eq!(badge.count(), 1);
        assert_eq!(badge.poll(start + Duration::from_secs(299), true), None);
        let second = polling(&mut badge, start + FEED_CHECK_EVERY);
        assert!(second > first);
        // A check due while one is still unanswered waits for it.
        assert_eq!(badge.poll(start + 2 * FEED_CHECK_EVERY, true), None);
        badge.unseen_answered(second, Some(2));
        assert_eq!(badge.count(), 2);
    }

    #[test]
    fn a_hidden_window_or_no_network_stops_polling_and_coming_back_asks_at_once() {
        let start = Instant::now();
        let mut badge = FeedBadge::default();
        let first = polling(&mut badge, start);
        badge.unseen_answered(first, Some(0));
        // Hidden (or offline): nothing is asked, however long it stays so.
        assert_eq!(badge.poll(start + Duration::from_secs(10), false), None);
        assert_eq!(badge.poll(start + 3 * FEED_CHECK_EVERY, false), None);
        assert!(!badge.polling);
        // Shown again a moment after the last check: asked at once anyway,
        // as TIDAL's `START_POLLING` does.
        let mut badge = FeedBadge::default();
        let first = polling(&mut badge, start);
        badge.unseen_answered(first, Some(0));
        assert_eq!(badge.poll(start + Duration::from_secs(5), false), None);
        polling(&mut badge, start + Duration::from_secs(6));
    }

    #[test]
    fn the_feed_seen_clears_the_count_at_once_and_a_lagging_answer_does_not_bring_it_back() {
        let start = Instant::now();
        let mut badge = FeedBadge::default();
        let first = polling(&mut badge, start);
        badge.unseen_answered(first, Some(1));
        // The Feed page asks for its rows; the page is not seen before they
        // answer (TIDAL's `status === "loaded"`).
        let page = badge.feed_requested();
        assert!(!badge.seen_due(true));
        assert!(badge.feed_answered(page, Some(1)));
        assert!(badge.seen_due(true));
        assert!(!badge.seen_due(false), "only on the Feed page");
        // Seen: 0 at once, and TIDAL is told.
        let put = badge.mark_seen(true).expect("told at once");
        assert_eq!(badge.count(), 0);
        assert!(!badge.seen_due(true));
        // A check due while the PUT is on its way waits for it.
        assert_eq!(badge.poll(start + FEED_CHECK_EVERY, true), None);
        // Answers asked for before the Feed was seen, arriving late, keep 0.
        badge.unseen_answered(first, Some(1));
        assert!(badge.feed_answered(page, Some(1)));
        assert_eq!(badge.count(), 0);
        badge.seen_answered(put, Ok(()));
        badge.unseen_answered(first, Some(1));
        assert_eq!(badge.count(), 0);
        // A check after TIDAL took it tells today's count.
        let after = polling(&mut badge, start + FEED_CHECK_EVERY);
        badge.unseen_answered(after, Some(3));
        assert_eq!(badge.count(), 3);
    }

    #[test]
    fn a_seen_the_network_did_not_carry_is_told_again_before_the_next_check() {
        let start = Instant::now();
        let mut badge = FeedBadge::default();
        let first = polling(&mut badge, start);
        badge.unseen_answered(first, Some(2));
        let page = badge.feed_requested();
        badge.feed_answered(page, Some(2));
        let put = badge.mark_seen(true).expect("told at once");
        // No network: owed, and the count stays 0 meanwhile.
        badge.seen_answered(put, Err(true));
        assert_eq!(badge.poll(start + FEED_CHECK_EVERY, false), None);
        let Some(FeedCall::TellSeen(again)) =
            badge.poll(start + FEED_CHECK_EVERY + Duration::from_secs(1), true)
        else {
            panic!("the owed PUT goes first");
        };
        assert_eq!(badge.count(), 0);
        badge.seen_answered(again, Ok(()));
        // Then the check polling's start asked for.
        let check = polling(
            &mut badge,
            start + FEED_CHECK_EVERY + Duration::from_secs(2),
        );
        badge.unseen_answered(check, Some(0));
        assert_eq!(badge.count(), 0);
        // Seen while TIDAL cannot be asked at all: owed from the start.
        let mut badge = FeedBadge::default();
        let first = polling(&mut badge, start);
        badge.unseen_answered(first, Some(1));
        let page = badge.feed_requested();
        badge.feed_answered(page, Some(1));
        // The network goes (or offline mode is turned on).
        assert_eq!(badge.poll(start + Duration::from_millis(500), false), None);
        assert_eq!(badge.mark_seen(false), None);
        assert_eq!(badge.count(), 0);
        let Some(FeedCall::TellSeen(put)) = badge.poll(start + Duration::from_secs(1), true) else {
            panic!("the owed PUT goes once TIDAL can be asked");
        };
        // A refusal is given up, as TIDAL's `allow40X` does.
        badge.seen_answered(put, Err(false));
        assert!(matches!(
            badge.poll(start + Duration::from_secs(2), true),
            Some(FeedCall::Check(_))
        ));
    }

    #[test]
    fn another_account_starts_from_nothing_and_the_last_ones_answers_are_dropped() {
        let start = Instant::now();
        let mut badge = FeedBadge::default();
        let check = polling(&mut badge, start);
        let page = badge.feed_requested();
        badge.reset();
        badge.unseen_answered(check, Some(4));
        assert!(!badge.feed_answered(page, Some(4)));
        assert_eq!(badge.count(), 0);
        // The new account's polling asks at once.
        let next = polling(&mut badge, start + Duration::from_secs(1));
        badge.unseen_answered(next, Some(1));
        assert_eq!(badge.count(), 1);
        // The last account's PUT failing for want of network leaves nothing
        // owed for this one.
        let mut badge = FeedBadge::default();
        let check = polling(&mut badge, start);
        badge.unseen_answered(check, Some(1));
        let page = badge.feed_requested();
        badge.feed_answered(page, Some(1));
        let put = badge.mark_seen(true).expect("told at once");
        badge.reset();
        badge.seen_answered(put, Err(true));
        assert!(matches!(
            badge.poll(start + Duration::from_secs(1), true),
            Some(FeedCall::Check(_))
        ));
    }

    #[test]
    fn only_a_refusal_gives_the_seen_up() {
        assert!(tell_again(&ApiError::Network("down".into())));
        assert!(tell_again(&ApiError::RateLimited));
        assert!(tell_again(&ApiError::Status {
            status: 503,
            message: String::new()
        }));
        assert!(!tell_again(&ApiError::Status {
            status: 403,
            message: String::new()
        }));
        assert!(!tell_again(&ApiError::NotSignedIn));
    }

    // ---- the app ----------------------------------------------------------

    /// A signed-in app whose requests are recorded and never sent.
    fn signed_in(name: &str) -> App {
        let root = crate::paths::unique_test_dir(&format!("feed-badge-{name}"));
        let mut app = App::new(
            &crate::backend::Waker::default(),
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
        app.backend.set_offline(true);
        app.follows_reachability = false;
        app.auth = AuthStatus::Connected {
            username: "alice".into(),
        };
        app
    }

    /// One pass of the app's logic, its window showing.
    fn pass(app: &mut App) {
        let ctx = egui::Context::default();
        let mut output = ctx.run_ui(egui::RawInput::default(), |ui| {
            app.background_frame(ui.ctx());
        });
        output.textures_delta.clear();
    }

    fn checks(requests: &[ApiRequest]) -> Vec<u64> {
        requests
            .iter()
            .filter_map(|request| match request {
                ApiRequest::FeedUnseen { serial } => Some(*serial),
                _ => None,
            })
            .collect()
    }

    fn feed_loads(requests: &[ApiRequest]) -> Vec<u64> {
        requests
            .iter()
            .filter_map(|request| match request {
                ApiRequest::Feed { serial } => Some(*serial),
                _ => None,
            })
            .collect()
    }

    fn told(requests: &[ApiRequest]) -> Vec<u64> {
        requests
            .iter()
            .filter_map(|request| match request {
                ApiRequest::FeedSeen { serial } => Some(*serial),
                _ => None,
            })
            .collect()
    }

    fn feed(unseen: u32) -> crate::api::models::Feed {
        crate::api::models::Feed {
            entries: Vec::new(),
            unseen,
        }
    }

    /// The counter is asked for as the signed-in window's logic runs, once
    /// (the request on its way is shared), and its answer is the number;
    /// nothing is asked in offline mode or while the window is hidden.
    #[test]
    fn the_signed_in_app_asks_for_the_count_once_and_shows_the_answer() {
        let mut app = signed_in("asks");
        pass(&mut app);
        pass(&mut app);
        let sent = app.backend.take_feed_requests();
        let [serial] = checks(&sent)[..] else {
            panic!("one count check: {sent:?}");
        };
        assert!(feed_loads(&sent).is_empty(), "not the whole Feed: {sent:?}");
        assert_eq!(app.feed_unseen(), 0);
        app.handle_api(ApiResponse::FeedUnseen {
            serial,
            result: Ok(1),
        });
        assert_eq!(app.feed_unseen(), 1);
        // A failed check leaves the number as it was.
        app.feed_badge.checking = Some(serial + 1);
        app.handle_api(ApiResponse::FeedUnseen {
            serial: serial + 1,
            result: Err(ApiError::Network("down".into())),
        });
        assert_eq!(app.feed_unseen(), 1);
        app.backend.shutdown();

        let mut app = signed_in("offline-mode");
        let ctx = egui::Context::default();
        app.apply(Action::SetOfflineMode(true), &ctx);
        app.apply(Action::SettingsChanged, &ctx);
        assert!(!app.network_reachable);
        pass(&mut app);
        assert!(checks(&app.backend.take_feed_requests()).is_empty());
        app.backend.shutdown();

        let mut app = signed_in("hidden");
        app.window_hidden = true;
        pass(&mut app);
        assert!(checks(&app.backend.take_feed_requests()).is_empty());
        app.window_hidden = false;
        pass(&mut app);
        assert_eq!(checks(&app.backend.take_feed_requests()).len(), 1);
        app.backend.shutdown();

        let mut app = signed_in("signed-out");
        app.auth = AuthStatus::SignedOut;
        pass(&mut app);
        assert!(app.backend.take_feed_requests().is_empty());
        app.backend.shutdown();
    }

    /// TIDAL's Feed page: the counter goes the moment the page's rows have
    /// loaded with unseen activities, TIDAL is told once, and answers asked
    /// for before then do not bring the number back.
    #[test]
    fn opening_the_feed_clears_the_counter_tells_tidal_and_keeps_it_cleared() {
        let mut app = signed_in("seen");
        pass(&mut app);
        let check = checks(&app.backend.take_feed_requests())[0];
        app.handle_api(ApiResponse::FeedUnseen {
            serial: check,
            result: Ok(2),
        });
        assert_eq!(app.feed_unseen(), 2);

        // The Feed page asks for its rows; until they answer it is not seen.
        app.open(Page::Feed);
        let sent = app.backend.take_feed_requests();
        let [load] = feed_loads(&sent)[..] else {
            panic!("the Feed's rows asked for: {sent:?}");
        };
        assert!(told(&sent).is_empty(), "not before the rows load");
        assert_eq!(app.feed_unseen(), 2, "still counted while loading");
        app.handle_api(ApiResponse::Feed {
            serial: load,
            result: Ok(feed(2)),
        });
        assert_eq!(app.feed_unseen(), 0, "gone at once");
        assert!(matches!(app.feed, Loadable::Loaded(_)));
        let sent = app.backend.take_feed_requests();
        let [put] = told(&sent)[..] else {
            panic!("TIDAL told once: {sent:?}");
        };

        // Late answers from before keep it cleared, on the page or off it.
        app.handle_api(ApiResponse::FeedUnseen {
            serial: check,
            result: Ok(2),
        });
        app.handle_api(ApiResponse::Feed {
            serial: load,
            result: Ok(feed(2)),
        });
        assert_eq!(app.feed_unseen(), 0);
        app.open(Page::Home);
        app.handle_api(ApiResponse::Feed {
            serial: load,
            result: Ok(feed(2)),
        });
        assert_eq!(app.feed_unseen(), 0);
        app.handle_api(ApiResponse::FeedSeen {
            serial: put,
            result: Ok(()),
        });
        app.handle_api(ApiResponse::FeedUnseen {
            serial: check,
            result: Ok(2),
        });
        assert_eq!(app.feed_unseen(), 0);
        assert!(
            told(&app.backend.take_feed_requests()).is_empty(),
            "told once"
        );
        app.backend.shutdown();
    }

    /// Feed rows that failed to load are not seen, as TIDAL's
    /// `status === "loaded"` says; the counter stays.
    #[test]
    fn a_feed_that_failed_to_load_is_not_seen() {
        let mut app = signed_in("failed");
        pass(&mut app);
        let check = checks(&app.backend.take_feed_requests())[0];
        app.handle_api(ApiResponse::FeedUnseen {
            serial: check,
            result: Ok(1),
        });
        app.open(Page::Feed);
        let load = feed_loads(&app.backend.take_feed_requests())[0];
        app.handle_api(ApiResponse::Feed {
            serial: load,
            result: Err(ApiError::Network("down".into())),
        });
        assert_eq!(app.feed_unseen(), 1);
        assert!(told(&app.backend.take_feed_requests()).is_empty());
        app.backend.shutdown();
    }

    /// Signing out forgets the Feed and its counter; an answer asked for
    /// the last account changes nothing.
    #[test]
    fn signing_out_forgets_the_feed_and_its_counter() {
        let mut app = signed_in("sign-out");
        pass(&mut app);
        let check = checks(&app.backend.take_feed_requests())[0];
        app.handle_api(ApiResponse::FeedUnseen {
            serial: check,
            result: Ok(1),
        });
        app.open(Page::Feed);
        let load = feed_loads(&app.backend.take_feed_requests())[0];
        app.handle_backend_events(vec![crate::backend::Event::Auth(AuthStatus::SignedOut)]);
        assert_eq!(app.feed_unseen(), 0);
        app.handle_api(ApiResponse::Feed {
            serial: load,
            result: Ok(feed(4)),
        });
        assert!(
            matches!(app.feed, Loadable::NotLoaded),
            "the last account's"
        );
        assert_eq!(app.feed_unseen(), 0);
        app.backend.shutdown();
    }
}
