mod support;
use kwel_native_host::{ipc, paths::RuntimePaths, server};
use kwel_protocol::{
    Code, EXTENSION_ID, Error, Item, LocalResponse, MAX_MESSAGE_BYTES, Message, Result,
};
use std::time::Duration;
use tokio::sync::mpsc;
const A: &str = "00000000-0000-4000-8000-00000000000a";
const B: &str = "00000000-0000-4000-8000-00000000000b";
const REF: &str = "00000000-0000-4000-8000-000000000101";
struct Host {
    _directory: tempfile::TempDir,
    paths: RuntimePaths,
    input: mpsc::Sender<Result<Message>>,
    output: mpsc::Receiver<Message>,
    task: tokio::task::JoinHandle<Result<()>>,
}
impl Host {
    async fn start(profile: &str) -> Self {
        let directory = support::directory();
        let paths =
            RuntimePaths::at(directory.path().canonicalize().unwrap().join("runtime")).unwrap();
        let (input, receiver) = mpsc::channel(4);
        let (sender, mut output) = mpsc::channel(32);
        let copy = paths.clone();
        let origin = format!("chrome-extension://{EXTENSION_ID}/");
        let task =
            tokio::spawn(
                async move { server::run_transport(&origin, copy, receiver, sender).await },
            );
        input
            .send(Ok(Message {
                profile_id: profile.into(),
                profile_label: "Fixture".into(),
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
        Self {
            _directory: directory,
            paths,
            input,
            output,
            task,
        }
    }
    async fn next(&mut self) -> Message {
        tokio::time::timeout(Duration::from_secs(3), self.output.recv())
            .await
            .unwrap()
            .unwrap()
    }
    async fn stop(self) {
        drop(self.input);
        assert!(
            tokio::time::timeout(Duration::from_secs(3), self.task)
                .await
                .unwrap()
                .unwrap()
                .is_ok()
        );
    }
}
async fn exchange(paths: RuntimePaths, profile: &str, request: serde_json::Value) -> LocalResponse {
    let mut connection = ipc::connect(&paths.endpoint(profile, false).unwrap())
        .await
        .unwrap();
    ipc::write(&mut *connection, &request).await.unwrap();
    let payload = ipc::read(&mut *connection, MAX_MESSAGE_BYTES)
        .await
        .unwrap();
    serde_json::from_slice(&payload).unwrap()
}
fn search(profile: &str, request: &str) -> serde_json::Value {
    serde_json::json!({"v":1,"type":"search","profileId":profile,"requestId":request,"query":"Kyoto 日本語 ! $()","limit":20})
}
fn result(id: &str) -> Message {
    Message {
        request_id: id.into(),
        has_more: Some(true),
        items: vec![Item {
            title: "Second-ranked".into(),
            url: "https://example.com/2".into(),
            domain: "example.com".into(),
            open_ref: REF.into(),
        }],
        ..Message::new("search-result")
    }
}

#[tokio::test]
async fn search_open_correlation_single_use_and_profile_isolation() {
    let mut host = Host::start(A).await;
    let paths = host.paths.clone();
    let task = tokio::spawn(exchange(paths, A, search(A, "same-client-id")));
    let request = host.next().await;
    assert_eq!(request.kind, "search");
    assert_eq!(request.query, "Kyoto 日本語 ! $()");
    assert_eq!(request.limit, 20);
    assert!(request.profile_id.is_empty());
    assert_ne!(request.request_id, "same-client-id");
    host.input
        .send(Ok(result(&request.request_id)))
        .await
        .unwrap();
    assert!(
        matches!(task.await.unwrap(),LocalResponse::Search{request_id,has_more:true,items,..} if request_id=="same-client-id"&&items[0].open_ref==REF)
    );
    let wrong = exchange(host.paths.clone(), A, search(B, "wrong")).await;
    assert!(matches!(
        wrong,
        LocalResponse::Error {
            code: Code::InvalidRequest,
            ..
        }
    ));
    let open = serde_json::json!({"v":1,"type":"open-page","profileId":A,"requestId":"open","openRef":REF});
    let task = tokio::spawn(exchange(host.paths.clone(), A, open.clone()));
    let request = host.next().await;
    assert_eq!(request.kind, "open-page");
    assert_eq!(request.open_ref, REF);
    assert!(request.profile_id.is_empty());
    host.input
        .send(Ok(Message {
            request_id: request.request_id,
            ok: true,
            ..Message::new("open-result")
        }))
        .await
        .unwrap();
    assert!(matches!(
        task.await.unwrap(),
        LocalResponse::Open { ok: true, .. }
    ));
    assert!(matches!(
        exchange(host.paths.clone(), A, open).await,
        LocalResponse::Error {
            code: Code::ResultExpired,
            ..
        }
    ));
    host.stop().await;
}
#[tokio::test]
async fn concurrent_clients_can_reuse_request_ids_without_mixing_responses() {
    let mut host = Host::start(A).await;
    let one = tokio::spawn(exchange(host.paths.clone(), A, search(A, "r")));
    let two = tokio::spawn(exchange(host.paths.clone(), A, search(A, "r")));
    let first = host.next().await;
    let second = host.next().await;
    assert_ne!(first.request_id, second.request_id);
    host.input
        .send(Ok(result(&second.request_id)))
        .await
        .unwrap();
    host.input
        .send(Ok(result(&first.request_id)))
        .await
        .unwrap();
    for task in [one, two] {
        assert!(
            matches!(task.await.unwrap(),LocalResponse::Search{request_id,..} if request_id=="r")
        );
    }
    host.stop().await;
}
#[tokio::test]
async fn disconnect_fails_pending_requests_and_removes_owned_endpoints() {
    let mut host = Host::start(A).await;
    let paths = host.paths.clone();
    let task = tokio::spawn(exchange(paths.clone(), A, search(A, "r")));
    host.next().await;
    host.stop().await;
    assert!(matches!(
        task.await.unwrap(),
        LocalResponse::Error {
            code: Code::Disconnected,
            ..
        }
    ));
    #[cfg(unix)]
    assert!(!paths.endpoint(A, false).unwrap().exists());
}
#[tokio::test]
async fn duplicate_profile_fails_closed_and_recovers_after_duplicate_disconnect() {
    let mut host = Host::start(A).await;
    let (input, receiver) = mpsc::channel(4);
    let (sender, mut output) = mpsc::channel(4);
    let paths = host.paths.clone();
    let second = tokio::spawn(async move {
        server::run_transport(
            &format!("chrome-extension://{EXTENSION_ID}/"),
            paths,
            receiver,
            sender,
        )
        .await
    });
    input
        .send(Ok(Message {
            profile_id: A.into(),
            ..Message::new("hello")
        }))
        .await
        .unwrap();
    let response = tokio::time::timeout(
        Duration::from_secs(3),
        exchange(host.paths.clone(), A, search(A, "conflict")),
    )
    .await
    .unwrap();
    assert!(matches!(
        response,
        LocalResponse::Error {
            code: Code::ProfileConflict,
            ..
        }
    ));
    assert!(output.try_recv().is_err());
    drop(input);
    assert!(matches!(
        second.await.unwrap(),
        Err(Error(Code::ProfileConflict))
    ));
    // A request that reached Chrome before the duplicate claim is deliberately
    // unanswered above. Discard that old frame, without a timing-based sleep.
    while host.output.try_recv().is_ok() {}
    let task = tokio::spawn(exchange(host.paths.clone(), A, search(A, "recovered")));
    let request = host.next().await;
    host.input
        .send(Ok(result(&request.request_id)))
        .await
        .unwrap();
    assert!(matches!(task.await.unwrap(), LocalResponse::Search { .. }));
    host.stop().await;
}
#[tokio::test]
async fn public_port_rejects_management_and_private_port_requires_capability() {
    let host = Host::start(A).await;
    for operation in ["uninstall", "profile-claim", "profile-release", "doctor"] {
        assert!(matches!(
            exchange(
                host.paths.clone(),
                A,
                serde_json::json!({"v":1,"type":operation,"requestId":"r","profileId":A})
            )
            .await,
            LocalResponse::Error {
                code: Code::InvalidRequest,
                ..
            }
        ));
    }
    let mut stream = ipc::connect(&host.paths.endpoint(A, true).unwrap())
        .await
        .unwrap();
    ipc::write(
        &mut *stream,
        &serde_json::json!({"token":"incorrect","operation":"claim","session":B}),
    )
    .await
    .unwrap();
    assert_eq!(ipc::read(&mut *stream, 128).await.unwrap(), b"false");
    host.stop().await;
}
#[tokio::test]
async fn unsafe_origin_never_starts_a_host() {
    let directory = support::directory();
    let paths = RuntimePaths::at(directory.path().canonicalize().unwrap().join("runtime")).unwrap();
    let (_input, receiver) = mpsc::channel(1);
    let (sender, _output) = mpsc::channel(1);
    assert!(matches!(
        server::run_transport(
            "chrome-extension://aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa/",
            paths,
            receiver,
            sender
        )
        .await,
        Err(Error(Code::PermissionRequired))
    ));
}
#[test]
fn stdio_uses_binary_framing_including_windows_control_z_header() {
    use std::{
        io::Write,
        process::{Command, Stdio},
    };
    let profile = uuid::Uuid::new_v4().to_string();
    let payload = serde_json::to_vec(&Message {
        profile_id: profile.clone(),
        profile_label: "Fixture".into(),
        ..Message::new("hello")
    })
    .unwrap();
    let mut padded = payload;
    while padded.len() % 256 != 0x1a {
        padded.push(b' ');
    }
    padded.extend_from_slice(b"\r\n");
    while padded.len() % 256 != 0x1a {
        padded.push(b' ');
    }
    let mut child = Command::new(env!("CARGO_BIN_EXE_kwel-native-host"))
        .arg(format!("chrome-extension://{EXTENSION_ID}/"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdin = child.stdin.take().unwrap();
    stdin
        .write_all(&(padded.len() as u32).to_le_bytes())
        .unwrap();
    stdin.write_all(&padded).unwrap();
    stdin.flush().unwrap();
    let bytes =
        kwel_protocol::read_frame(&mut child.stdout.take().unwrap(), MAX_MESSAGE_BYTES).unwrap();
    assert_eq!(kwel_protocol::decode(&bytes).unwrap().kind, "hello-ack");
    drop(stdin);
    let output = child.wait_with_output().unwrap();
    assert!(output.status.success());
    let diagnostics = String::from_utf8_lossy(&output.stderr);
    assert!(!diagnostics.contains(&profile));
    assert!(!diagnostics.contains("Fixture"));
}
