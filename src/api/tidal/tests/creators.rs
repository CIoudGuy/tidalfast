//! Creator names, against a scripted local server. Nothing here reaches
//! a real account.

use super::*;

#[tokio::test]
async fn a_creator_is_named_by_their_profile_or_by_nothing() {
    let (base, seen) = script(vec![
        reply(
            "200 OK",
            "",
            r#"{"userId":11110001,"name":" Robin K. ","handle":"robin"}"#,
        ),
        reply(
            "403 Forbidden",
            "",
            r#"{"status":403,"userMessage":"private"}"#,
        ),
        reply(
            "404 Not Found",
            "",
            r#"{"status":404,"userMessage":"gone"}"#,
        ),
        reply("200 OK", "", r#"{"userId":11110002,"name":"11110002"}"#),
        reply("500 Internal Server Error", "", "{}"),
    ])
    .await;
    let client = client(&base, refresher());
    assert_eq!(
        client.user_name("11110001").await.unwrap().as_deref(),
        Some("Robin K.")
    );
    // Private and deleted profiles are answers, not failures.
    assert_eq!(client.user_name("2").await.unwrap(), None);
    assert_eq!(client.user_name("3").await.unwrap(), None);
    // A profile whose name is the id itself names no one.
    assert_eq!(client.user_name("11110002").await.unwrap(), None);
    // A server failure is no answer: the caller asks again later.
    assert!(client.user_name("5").await.is_err());
    let seen = seen.lock().unwrap();
    assert!(seen[0].starts_with("GET /profiles/11110001"), "{}", seen[0]);
}
