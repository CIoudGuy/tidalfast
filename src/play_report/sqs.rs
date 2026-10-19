//! TIDAL's event collector speaks AWS SQS `SendMessageBatch`: a form with
//! up to ten numbered entries in, SQS XML out
//! (docs/_reference/tidal-activity.md, 2.2).

/// The most events one batch carries.
pub const MAX_BATCH: usize = 10;

/// One event ready to send.
pub struct Entry<'a> {
    pub id: &'a str,
    pub name: &'a str,
    pub body: String,
    pub headers: String,
}

/// The form fields for `entries` (at most [`MAX_BATCH`]; the rest are
/// left out). Ids must be distinct.
pub fn encode(entries: &[Entry<'_>]) -> Vec<(String, String)> {
    let mut form = Vec::with_capacity(entries.len().min(MAX_BATCH) * 9);
    for (index, entry) in entries.iter().take(MAX_BATCH).enumerate() {
        let prefix = format!("SendMessageBatchRequestEntry.{}", index + 1);
        let mut field = |key: &str, value: &str| {
            form.push((format!("{prefix}.{key}"), value.to_string()));
        };
        field("Id", entry.id);
        field("MessageBody", &entry.body);
        field("MessageAttribute.1.Name", "Name");
        field("MessageAttribute.1.Value.StringValue", entry.name);
        field("MessageAttribute.1.Value.DataType", "String");
        field("MessageAttribute.2.Name", "Headers");
        field("MessageAttribute.2.Value.StringValue", &entry.headers);
        field("MessageAttribute.2.Value.DataType", "String");
    }
    form
}

/// What became of each event of a batch.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Outcome {
    /// Accepted: remove from the queue.
    pub accepted: Vec<String>,
    /// Malformed (`SenderFault=true`): drop, sending again cannot help.
    pub rejected: Vec<String>,
    /// Failed on the collector's side: keep and try again later.
    pub retry: Vec<String>,
}

/// Why a whole batch failed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Failure {
    /// Sending again later may work (5xx, throttling, 401/403, unknown).
    Retry(String),
    /// The batch itself was malformed; dropping it is the only way on.
    Reject(String),
}

fn child_text<'a>(node: roxmltree::Node<'a, 'a>, name: &str) -> Option<&'a str> {
    node.children()
        .find(|child| child.tag_name().name() == name)
        .and_then(|child| child.text())
        .map(str::trim)
}

/// Judges the collector's answer to a batch of `ids`.
pub fn judge(status: u16, body: &str, ids: &[String]) -> Result<Outcome, Failure> {
    let document = roxmltree::Document::parse(body).ok();
    if !(200..300).contains(&status) {
        let code = document
            .as_ref()
            .and_then(|document| {
                document
                    .descendants()
                    .find(|node| node.tag_name().name() == "Error")
            })
            .and_then(|error| child_text(error, "Code").map(str::to_string))
            .unwrap_or_default();
        let reason = format!("HTTP {status} {code}").trim().to_string();
        let malformed = code.contains("BatchEntryIdsNotDistinct")
            || code.contains("InvalidBatchEntryId")
            || code.contains("TooManyEntriesInBatchRequest")
            || code.contains("EmptyBatchRequest")
            || code.contains("BatchRequestTooLong");
        return Err(if malformed || status == 400 && !code.is_empty() {
            Failure::Reject(reason)
        } else {
            Failure::Retry(reason)
        });
    }
    let mut outcome = Outcome::default();
    if let Some(document) = &document {
        for node in document.descendants() {
            match node.tag_name().name() {
                "SendMessageBatchResultEntry" => {
                    if let Some(id) = child_text(node, "Id") {
                        outcome.accepted.push(id.to_string());
                    }
                }
                "BatchResultErrorEntry" => {
                    let Some(id) = child_text(node, "Id") else {
                        continue;
                    };
                    if child_text(node, "SenderFault").is_some_and(|fault| fault == "true") {
                        outcome.rejected.push(id.to_string());
                    } else {
                        outcome.retry.push(id.to_string());
                    }
                }
                _ => {}
            }
        }
    }
    let mentioned = |id: &String| {
        outcome.accepted.contains(id) || outcome.rejected.contains(id) || outcome.retry.contains(id)
    };
    let unmentioned: Vec<String> = ids.iter().filter(|id| !mentioned(id)).cloned().collect();
    // A success that names no entry at all is taken as all accepted:
    // sending the same plays again would count them twice.
    outcome.accepted.extend(unmentioned);
    outcome.accepted.retain(|id| ids.contains(id));
    outcome.rejected.retain(|id| ids.contains(id));
    outcome.retry.retain(|id| ids.contains(id));
    Ok(outcome)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_are_numbered_from_one_with_both_attributes() {
        let entries: Vec<Entry<'_>> = ["a", "b"]
            .iter()
            .map(|id| Entry {
                id,
                name: "playback_session",
                body: format!("{{\"uuid\":\"{id}\"}}"),
                headers: "{}".into(),
            })
            .collect();
        let form = encode(&entries);
        assert_eq!(form.len(), 16);
        assert_eq!(
            form[0],
            ("SendMessageBatchRequestEntry.1.Id".into(), "a".into())
        );
        assert_eq!(
            form[1],
            (
                "SendMessageBatchRequestEntry.1.MessageBody".into(),
                "{\"uuid\":\"a\"}".into()
            )
        );
        assert_eq!(
            form[3].0,
            "SendMessageBatchRequestEntry.1.MessageAttribute.1.Value.StringValue"
        );
        assert_eq!(form[3].1, "playback_session");
        assert_eq!(form[5].1, "Headers");
        assert_eq!(form[8].0, "SendMessageBatchRequestEntry.2.Id");
        // Reqwest form-encodes the pairs; the JSON survives the round trip.
        let encoded = serde_urlencoded_like(&form);
        assert!(encoded.contains("SendMessageBatchRequestEntry.2.MessageBody=%7B%22uuid%22"));
    }

    fn serde_urlencoded_like(form: &[(String, String)]) -> String {
        url::form_urlencoded::Serializer::new(String::new())
            .extend_pairs(form)
            .finish()
    }

    #[test]
    fn at_most_ten_entries_go_in_a_batch() {
        let ids: Vec<String> = (0..12).map(|index| index.to_string()).collect();
        let entries: Vec<Entry<'_>> = ids
            .iter()
            .map(|id| Entry {
                id,
                name: "n",
                body: String::new(),
                headers: String::new(),
            })
            .collect();
        assert_eq!(encode(&entries).len(), 10 * 8);
    }

    #[test]
    fn the_answer_sorts_events_into_accepted_dropped_and_retried() {
        let xml = r#"<?xml version="1.0"?>
<SendMessageBatchResponse xmlns="http://queue.amazonaws.com/doc/2012-11-05/">
  <SendMessageBatchResult>
    <SendMessageBatchResultEntry><Id>a</Id><MessageId>m</MessageId></SendMessageBatchResultEntry>
    <BatchResultErrorEntry><Id>b</Id><SenderFault>true</SenderFault><Code>InvalidMessageContents</Code></BatchResultErrorEntry>
    <BatchResultErrorEntry><Id>c</Id><SenderFault>false</SenderFault><Code>InternalError</Code></BatchResultErrorEntry>
  </SendMessageBatchResult>
</SendMessageBatchResponse>"#;
        let ids = vec!["a".to_string(), "b".into(), "c".into()];
        let outcome = judge(200, xml, &ids).unwrap();
        assert_eq!(outcome.accepted, ["a"]);
        assert_eq!(outcome.rejected, ["b"]);
        assert_eq!(outcome.retry, ["c"]);
        let all = judge(200, "", &ids).unwrap();
        assert_eq!(all.accepted, ids);
    }

    #[test]
    fn failed_batches_are_retried_unless_malformed() {
        let ids = vec!["a".to_string()];
        let duplicate = "<ErrorResponse><Error><Type>Sender</Type><Code>AWS.SimpleQueueService.BatchEntryIdsNotDistinct</Code></Error></ErrorResponse>";
        assert!(matches!(
            judge(400, duplicate, &ids),
            Err(Failure::Reject(_))
        ));
        assert!(matches!(judge(503, "", &ids), Err(Failure::Retry(_))));
        assert!(matches!(judge(401, "", &ids), Err(Failure::Retry(_))));
        assert!(matches!(judge(400, "", &ids), Err(Failure::Retry(_))));
    }
}
