use kwel_client::Client;
use kwel_native_host::{paths::RuntimePaths, server};
use kwel_protocol::{Code, EXTENSION_ID, Error, Item, LocalRequest, LocalResponse, Message};
use std::time::Duration;
use tokio::sync::mpsc;
const A: &str = "00000000-0000-4000-8000-00000000000a";
const B: &str = "00000000-0000-4000-8000-00000000000b";

#[tokio::test]
async fn discovery_search_open_and_disconnect_keep_two_profiles_separate() {
    #[cfg(target_os = "macos")]
    let parent = std::path::PathBuf::from("/private/tmp");
    #[cfg(target_os = "linux")]
    let parent = std::path::PathBuf::from("/tmp");
    #[cfg(windows)]
    let parent = std::env::temp_dir();
    let directory = tempfile::Builder::new()
        .prefix("kwcl-")
        .tempdir_in(parent)
        .unwrap();
    let paths = RuntimePaths::at(directory.path().canonicalize().unwrap().join("runtime")).unwrap();
    let mut sessions = Vec::new();
    for profile in [A, B] {
        let (input, receiver) = mpsc::channel(4);
        let (sender, mut output) = mpsc::channel(32);
        let copy = paths.clone();
        let task = tokio::spawn(async move {
            server::run_transport(
                &format!("chrome-extension://{EXTENSION_ID}/"),
                copy,
                receiver,
                sender,
            )
            .await
        });
        input
            .send(Ok(Message {
                profile_id: profile.into(),
                ..Message::new("hello")
            }))
            .await
            .unwrap();
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(3), output.recv())
                .await
                .unwrap()
                .unwrap()
                .kind,
            "hello-ack"
        );
        sessions.push((input, output, task));
    }
    let client = Client { paths };
    let copy = client.clone();
    let discovery = tokio::spawn(async move { copy.profiles().await });
    for (input, output, _) in &mut sessions {
        assert_eq!(
            tokio::time::timeout(Duration::from_secs(3), output.recv())
                .await
                .unwrap()
                .unwrap()
                .kind,
            "ping"
        );
        input.send(Ok(Message::new("pong"))).await.unwrap();
    }
    assert_eq!(
        discovery
            .await
            .unwrap()
            .unwrap()
            .iter()
            .map(|profile| profile.profile_id.as_str())
            .collect::<Vec<_>>(),
        [A, B]
    );
    for (index, profile) in [A, B].into_iter().enumerate() {
        let copy = client.clone();
        let search = tokio::spawn(async move {
            copy.request(LocalRequest::Search {
                v: 1,
                request_id: "local".into(),
                profile_id: profile.into(),
                query: "Kyoto".into(),
                limit: 20,
            })
            .await
        });
        let (input, output, _) = &mut sessions[index];
        let request = tokio::time::timeout(Duration::from_secs(3), output.recv())
            .await
            .unwrap()
            .unwrap();
        let reference = format!("00000000-0000-4000-8000-{:012}", index + 100);
        input
            .send(Ok(Message {
                request_id: request.request_id,
                has_more: Some(false),
                items: vec![Item {
                    title: format!("Result {index}"),
                    url: "https://example.com/".into(),
                    open_ref: reference.clone(),
                    ..Item::default()
                }],
                ..Message::new("search-result")
            }))
            .await
            .unwrap();
        assert!(
            matches!(search.await.unwrap().unwrap(),LocalResponse::Search {items,..} if items[0].open_ref==reference)
        );
    }
    // A cannot consume B's reference, even while both profiles are connected.
    assert!(matches!(
        client
            .request(LocalRequest::Open {
                v: 1,
                request_id: "open".into(),
                profile_id: A.into(),
                open_ref: "00000000-0000-4000-8000-000000000101".into()
            })
            .await,
        Err(Error(Code::ResultExpired))
    ));
    for (_, output, _) in &mut sessions {
        assert!(output.try_recv().is_err());
    }
    let (input_a, _output_a, task_a) = sessions.remove(0);
    drop(input_a);
    task_a.await.unwrap().unwrap();
    assert!(
        client
            .request(LocalRequest::Search {
                v: 1,
                request_id: "missing".into(),
                profile_id: A.into(),
                query: "Kyoto".into(),
                limit: 20
            })
            .await
            .is_err()
    );
    let (input_b, mut output_b, task_b) = sessions.pop().unwrap();
    assert!(output_b.try_recv().is_err());
    let discovery = tokio::spawn(async move { client.profiles().await });
    assert_eq!(
        tokio::time::timeout(Duration::from_secs(3), output_b.recv())
            .await
            .unwrap()
            .unwrap()
            .kind,
        "ping"
    );
    input_b.send(Ok(Message::new("pong"))).await.unwrap();
    let remaining = discovery.await.unwrap().unwrap();
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0].profile_id, B);
    drop(input_b);
    task_b.await.unwrap().unwrap();
}
