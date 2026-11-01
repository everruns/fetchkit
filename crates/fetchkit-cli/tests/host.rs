//! `fetchkit_cli::run` with a custom `CliHost`: the embedding contract.
//!
//! Proves an embedder (e.g. the bashkit `fetchkit` builtin) gets the same CLI
//! as the binary, with output, env, transport, and refused flags all routed
//! through the host. No test here touches the network.

use async_trait::async_trait;
use bytes::Bytes;
use fetchkit::{HttpTransport, ToolBuilder, TransportError, TransportRequest, TransportResponse};
use fetchkit_cli::{run, CliHost, HostFeature};
use std::io;
use std::process::Command;
use std::sync::{Arc, Mutex};

/// Records stdout/stderr; refuses features listed in `refused`.
#[derive(Default)]
struct TestHost {
    stdout: Mutex<String>,
    stderr: Mutex<String>,
    refused: Vec<HostFeature>,
    transport: Option<Arc<dyn HttpTransport>>,
    broken_pipe: bool,
}

impl TestHost {
    fn out(&self) -> String {
        self.stdout.lock().unwrap().clone()
    }
    fn err(&self) -> String {
        self.stderr.lock().unwrap().clone()
    }
}

#[async_trait]
impl CliHost for TestHost {
    fn write_stdout(&self, text: &str) -> io::Result<()> {
        if self.broken_pipe {
            return Err(io::Error::from(io::ErrorKind::BrokenPipe));
        }
        self.stdout.lock().unwrap().push_str(text);
        Ok(())
    }
    fn write_stderr(&self, text: &str) {
        self.stderr.lock().unwrap().push_str(text);
    }
    fn env_var(&self, _name: &str) -> Option<String> {
        None
    }
    fn configure_tool(&self, builder: ToolBuilder) -> ToolBuilder {
        match &self.transport {
            // The host owns egress policy; no host DNS resolution in tests.
            Some(t) => builder.transport(t.clone()).block_private_ips(false),
            None => builder,
        }
    }
    fn refuse(&self, feature: HostFeature) -> Option<String> {
        self.refused
            .contains(&feature)
            .then(|| "is not available in this environment".to_string())
    }
}

/// Serves one HTML page for every request and counts calls.
struct PageTransport {
    calls: Mutex<Vec<String>>,
}

#[async_trait]
impl HttpTransport for PageTransport {
    async fn execute(&self, req: TransportRequest) -> Result<TransportResponse, TransportError> {
        self.calls.lock().unwrap().push(req.url.to_string());
        let body = Bytes::from_static(b"<html><head><title>Hi</title></head><body><h1>Hello</h1><p>From the host transport.</p></body></html>");
        Ok(TransportResponse {
            status: 200,
            url: req.url,
            headers: vec![("content-type".into(), "text/html".into())],
            body: Box::pin(futures::stream::once(async move { Ok(body) })),
        })
    }
}

async fn run_with(host: &TestHost, args: &[&str]) -> i32 {
    run(
        std::iter::once("fetchkit").chain(args.iter().copied()),
        host,
    )
    .await
}

fn binary(args: &[&str]) -> std::process::Output {
    Command::new(env!("CARGO_BIN_EXE_fetchkit"))
        .args(args)
        .output()
        .expect("run fetchkit binary")
}

#[tokio::test]
async fn help_and_version_match_binary_byte_for_byte() {
    for args in [
        &["--help"][..],
        &["fetch", "--help"],
        &["mcp", "--help"],
        &["--version"],
    ] {
        let host = TestHost::default();
        assert_eq!(run_with(&host, args).await, 0, "{args:?}");
        let bin = binary(args);
        assert_eq!(bin.status.code(), Some(0), "{args:?}");
        assert_eq!(host.out(), String::from_utf8_lossy(&bin.stdout), "{args:?}");
        assert!(host.err().is_empty());
    }
}

#[tokio::test]
async fn usage_error_matches_binary_and_exits_2() {
    let args = ["fetch", "--no-such-flag", "https://example.com"];
    let host = TestHost::default();
    assert_eq!(run_with(&host, &args).await, 2);
    let bin = binary(&args);
    assert_eq!(bin.status.code(), Some(2));
    assert_eq!(host.err(), String::from_utf8_lossy(&bin.stderr));
    assert!(host.out().is_empty());
}

#[tokio::test]
async fn no_subcommand_prints_usage_and_exits_1() {
    let host = TestHost::default();
    assert_eq!(run_with(&host, &[]).await, 1);
    assert!(host.err().starts_with("Usage: fetchkit fetch <URL>"));
}

#[tokio::test]
async fn llmtxt_goes_to_host_stdout() {
    let host = TestHost::default();
    assert_eq!(run_with(&host, &["--llmtxt"]).await, 0);
    assert!(!host.out().is_empty());
}

#[tokio::test]
async fn fetch_uses_host_transport_and_writes_frontmatter() {
    let transport = Arc::new(PageTransport {
        calls: Mutex::new(Vec::new()),
    });
    let host = TestHost {
        transport: Some(transport.clone()),
        ..Default::default()
    };
    assert_eq!(
        run_with(&host, &["fetch", "http://site.test/page"]).await,
        0,
        "stderr: {}",
        host.err()
    );
    let out = host.out();
    assert!(
        out.starts_with("---\nurl: \"http://site.test/page\"\n"),
        "{out}"
    );
    assert!(out.contains("# Hello"), "{out}");
    // Every request (page + agent discovery probes) went through the host.
    let calls = transport.calls.lock().unwrap();
    assert_eq!(calls[0], "http://site.test/page");
    assert!(calls.iter().all(|u| u.starts_with("http://site.test/")));
}

#[tokio::test]
async fn fetch_json_output_through_host() {
    let host = TestHost {
        transport: Some(Arc::new(PageTransport {
            calls: Mutex::new(Vec::new()),
        })),
        ..Default::default()
    };
    assert_eq!(
        run_with(&host, &["fetch", "http://site.test/", "-o", "json"]).await,
        0
    );
    let json: serde_json::Value = serde_json::from_str(&host.out()).unwrap();
    assert_eq!(json["status_code"], 200);
}

#[tokio::test]
async fn refused_flags_fail_before_any_request() {
    let cases: [(&[&str], HostFeature); 4] = [
        (&["--allow-env-proxy"], HostFeature::EnvProxy),
        (&["--render-rakers"], HostFeature::RenderRakers),
        (&["--bot-auth-key", "x"], HostFeature::BotAuth),
        (
            &["--allow-remote-conversion"],
            HostFeature::RemoteConversion,
        ),
    ];
    for (flags, feature) in cases {
        let transport = Arc::new(PageTransport {
            calls: Mutex::new(Vec::new()),
        });
        let host = TestHost {
            refused: vec![feature],
            transport: Some(transport.clone()),
            ..Default::default()
        };
        let mut args = vec!["fetch", "http://site.test/"];
        args.extend_from_slice(flags);
        assert_eq!(run_with(&host, &args).await, 1, "{flags:?}");
        assert_eq!(
            host.err(),
            format!(
                "Error: {} is not available in this environment\n",
                feature.flag()
            )
        );
        assert!(host.out().is_empty());
        assert!(transport.calls.lock().unwrap().is_empty(), "{flags:?}");
    }
}

#[tokio::test]
async fn bot_auth_agent_alone_is_refused_too() {
    let host = TestHost {
        refused: vec![HostFeature::BotAuth],
        ..Default::default()
    };
    let code = run_with(
        &host,
        &["fetch", "http://site.test/", "--bot-auth-agent", "a.test"],
    )
    .await;
    assert_eq!(code, 1);
    assert!(host.err().contains("--bot-auth-key"));
}

#[tokio::test]
async fn mcp_is_refused_by_default_host() {
    let host = TestHost::default();
    assert_eq!(run_with(&host, &["mcp"]).await, 1);
    assert_eq!(
        host.err(),
        "Error: mcp is not available in this environment\n"
    );
}

#[tokio::test]
async fn broken_pipe_on_stdout_exits_0() {
    let host = TestHost {
        broken_pipe: true,
        ..Default::default()
    };
    assert_eq!(run_with(&host, &["--help"]).await, 0);
    assert!(host.err().is_empty());
}

#[tokio::test]
async fn fetch_error_is_reported_once_with_exit_1() {
    // Default DNS policy + unresolvable host: fails inside fetchkit, no panic.
    let host = TestHost::default();
    assert_eq!(run_with(&host, &["fetch", "not a url"]).await, 1);
    assert!(host.err().starts_with("Error: "), "{}", host.err());
}

#[test]
fn run_future_is_send() {
    // Embedders run the CLI inside `Send` async contexts (bashkit builtins).
    fn assert_send<T: Send>(_: T) {}
    let host = TestHost::default();
    assert_send(run(["fetchkit", "--help"], &host));
}
