//! Save for Later: the list, the optimistic toggle, and the rows' details.
//! The state is `crate::save_later::SaveLater`.

use super::*;
use crate::save_later::{Saved, parse_uri};

/// Rows whose details are asked for at once.
const DETAIL_BATCH: usize = 60;

impl App {
    /// Whether the item is set aside; `None` until the list is read.
    pub fn is_save_later(&self, uri: &str) -> Option<bool> {
        self.save_later.is_saved(uri)
    }

    /// Reads the list once a session (at sign-in), so every menu can tell
    /// from the set of saved ids, with no request of its own, whether the
    /// item reads "Remove from Save for Later". The page asks again when it
    /// opens.
    pub fn ensure_save_later(&mut self) {
        let state = &mut self.save_later;
        if state.loaded || state.loading || state.error.is_some() {
            return;
        }
        state.loading = true;
        self.backend.api(ApiRequest::SaveLater { cursor: None });
    }

    /// Reads the list again from the start, keeping what is shown until
    /// the answer comes.
    pub(super) fn reload_save_later(&mut self) {
        let state = &mut self.save_later;
        state.error = None;
        state.loading = true;
        // Rows that could not be read are asked for again.
        let missing: Vec<String> = state
            .asked
            .iter()
            .filter(|uri| !state.details.contains_key(*uri))
            .cloned()
            .collect();
        for uri in missing {
            state.asked.remove(&uri);
        }
        state.revision += 1;
        self.backend.api(ApiRequest::SaveLater { cursor: None });
    }

    /// Asks for the rows of the open tab that have no details yet, a batch
    /// at a time (the next goes when the last one is answered). Called by
    /// events (the page opening, a tab, an answer), never by drawing.
    pub fn request_save_later_details(&mut self) {
        let tab = self.save_later.tab;
        let wanted: Vec<String> = self
            .save_later
            .unasked(tab)
            .into_iter()
            .take(DETAIL_BATCH)
            .collect();
        for uri in wanted {
            self.save_later.asked.insert(uri.clone());
            self.save_later.pending += 1;
            self.backend.api(ApiRequest::SaveLaterDetail { uri });
        }
    }

    /// Reads Purchases once; a failure waits for Try again.
    pub fn ensure_purchases(&mut self) {
        if matches!(self.purchases, Loadable::NotLoaded) {
            self.purchases = Loadable::Loading;
            self.backend.api(ApiRequest::Purchases);
        }
    }

    /// The user's choice, shown at once; the write follows.
    pub(super) fn toggle_save_later(&mut self, uri: String, entry: Option<Box<Saved>>) {
        if parse_uri(&uri).is_none() {
            return;
        }
        let saved = !self.save_later.is_saved(&uri).unwrap_or(false);
        self.save_later.set_local(&uri, saved);
        self.save_later.wishes.insert(uri.clone(), saved);
        if let Some(entry) = entry {
            self.save_later.insert_detail(uri.clone(), *entry);
        }
        let message = if saved {
            gettext(self.locale, "Added to Save for Later")
        } else {
            gettext(self.locale, "Removed from Save for Later")
        };
        self.confirm(format!("savelater:{uri}"), message.into_owned(), None);
        if saved && *self.page() == Page::SaveForLater {
            self.request_save_later_details();
        }
        self.send_save_later(&uri);
    }

    /// Sends the newest wish for the item unless an older write is still
    /// unanswered: TIDAL runs requests in any order, so they go one at a
    /// time and the newer wish follows the answer.
    fn send_save_later(&mut self, uri: &str) {
        let Some(saved) = self.save_later.wishes.get(uri).copied() else {
            return;
        };
        if !self.save_later.in_flight.insert(uri.to_string()) {
            return;
        }
        self.backend.api(ApiRequest::SetSaveLater {
            uri: uri.to_string(),
            saved,
        });
    }

    pub(super) fn handle_save_later_response(&mut self, response: ApiResponse) {
        match response {
            ApiResponse::SaveLater { first, result } => match result {
                Ok(page) => {
                    let uris = page.items.iter().map(|(kind, id)| kind.uri(id)).collect();
                    let next = page.next.filter(|_| !page.items.is_empty());
                    self.save_later.absorb(first, uris, next.clone());
                    if let Some(cursor) = next {
                        self.save_later.loading = true;
                        self.backend.api(ApiRequest::SaveLater {
                            cursor: Some(cursor),
                        });
                    } else if *self.page() == Page::SaveForLater {
                        self.request_save_later_details();
                    }
                }
                Err(error) => {
                    self.save_later.loading = false;
                    self.save_later.error = Some(friendly_api_error(self.locale, &error));
                }
            },
            ApiResponse::SaveLaterChanged { uri, saved, result } => {
                self.save_later.in_flight.remove(&uri);
                let wish = self.save_later.wishes.get(&uri).copied();
                let key = format!("savelater:{uri}");
                match result {
                    Ok(()) => {
                        if wish == Some(saved) {
                            self.save_later.wishes.remove(&uri);
                            self.confirm_done(&key);
                        } else {
                            self.send_save_later(&uri);
                        }
                    }
                    Err(error) => {
                        if wish == Some(saved) {
                            // Put it back as it was.
                            self.save_later.wishes.remove(&uri);
                            self.save_later.set_local(&uri, !saved);
                            let message = gettext(
                                self.locale,
                                // Translators: {error} is an error message.
                                "Couldn't update Save for Later: {error}",
                            )
                            .replace("{error}", &error.to_string());
                            self.confirm_failed(&key, message);
                        } else {
                            self.send_save_later(&uri);
                        }
                    }
                }
            }
            ApiResponse::SaveLaterDetail { uri, result } => {
                self.save_later.pending = self.save_later.pending.saturating_sub(1);
                if let Ok(saved) = result {
                    self.save_later.insert_detail(uri, saved);
                }
                if self.save_later.pending == 0 && *self.page() == Page::SaveForLater {
                    self.request_save_later_details();
                }
            }
            ApiResponse::Purchases { result } => {
                self.purchases = match result {
                    Ok(purchases) => Loadable::Loaded(std::sync::Arc::new(purchases)),
                    Err(error) => Loadable::Failed(friendly_api_error(self.locale, &error)),
                };
            }
            _ => {}
        }
    }
}
