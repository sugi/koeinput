use anyhow::{Context, Result};
use futures_util::StreamExt;
use koeinput_dbus_api::Recognizer1Proxy;
use std::collections::HashMap;
use std::io::Write;
use std::time::Instant;
use tokio::io::{AsyncBufReadExt, BufReader};

const NOT_RUNNING: &str =
    "daemon not running (start it with: systemctl --user start koeinput, or run koeinput-daemon)";

fn runtime() -> Result<tokio::runtime::Runtime> {
    Ok(tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()?)
}

pub fn status() -> Result<()> {
    runtime()?.block_on(async {
        let conn = zbus::Connection::session()
            .await
            .context("no session bus")?;
        let p = Recognizer1Proxy::new(&conn).await?;
        let ready = p.ready().await.context(NOT_RUNNING)?;
        println!("ready: {}", if ready { "yes" } else { "no" });
        println!("model: {}", p.model().await?);
        println!("version: {}", p.version().await?);
        Ok(())
    })
}

pub fn run() -> Result<()> {
    let rt = runtime()?;
    let result = rt.block_on(listen());
    // The stdin read runs on a blocking-pool thread that only ends on input; dropping the runtime would wait for it.
    rt.shutdown_background();
    result
}

async fn listen() -> Result<()> {
    let conn = zbus::Connection::session()
        .await
        .context("no session bus")?;
    let p = Recognizer1Proxy::new(&conn).await?;
    let ready = p.ready().await.context(NOT_RUNNING)?;
    if !ready {
        let err = p
            .start(HashMap::new())
            .await
            .err()
            .map(|e| e.to_string())
            .unwrap_or_default();
        anyhow::bail!("daemon not ready: {err}");
    }
    let mut states = p.receive_state_changed().await?;
    let mut partials = p.receive_partial().await?;
    let mut finals = p.receive_final_text().await?;
    let mut errors = p.receive_error().await?;
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    let mut current: Option<u32> = None;
    let mut ended: Option<u32> = None;
    let mut stopped_at: Option<Instant> = None;
    println!("Enter: start / stop, Ctrl-C: cancel, q + Enter: quit");
    // Errors inside the loop must not skip the cancel below.
    let result: Result<()> = async {
        loop {
            tokio::select! {
                line = lines.next_line() => {
                    let Some(line) = line? else { break };
                    if line.trim() == "q" { break; }
                    match current {
                        None => match p.start(HashMap::new()).await {
                            Ok(id) => {
                                current = Some(id);
                                stopped_at = None;
                                println!("[preparing]");
                            }
                            Err(e) => println!("cannot start: {e}"),
                        },
                        Some(id) => {
                            stopped_at = Some(Instant::now());
                            if let Err(e) = p.stop(id).await {
                                stopped_at = None;
                                println!("cannot stop: {e}");
                            }
                        }
                    }
                }
                Some(s) = states.next() => {
                    let a = s.args()?;
                    if Some(a.session) == current && a.state == "recording" {
                        println!("[recording]");
                    }
                    if Some(a.session) == current && matches!(a.state.as_str(), "done" | "cancelled" | "error") {
                        ended = current.take();
                        if a.state != "done" {
                            stopped_at = None;
                        }
                    }
                }
                Some(s) = partials.next() => {
                    let a = s.args()?;
                    if Some(a.session) == current {
                        print!("\r\x1b[K{}", a.text);
                        std::io::stdout().flush()?;
                    }
                }
                Some(s) = finals.next() => {
                    let a = s.args()?;
                    if Some(a.session) == current || Some(a.session) == ended {
                        let after = stopped_at.take().map(|t| t.elapsed().as_secs_f64()).unwrap_or(0.0);
                        println!("\r\x1b[K{}", a.text);
                        println!("(final {after:.2} s after stop)");
                    }
                }
                Some(s) = errors.next() => {
                    let a = s.args()?;
                    if Some(a.session) == current || Some(a.session) == ended {
                        println!("\r\x1b[Kerror [{}]: {}", a.code, a.message);
                    }
                }
                _ = tokio::signal::ctrl_c() => break,
            }
        }
        Ok(())
    }
    .await;
    if let Some(id) = current {
        let _ = p.cancel(id).await;
    }
    result
}
