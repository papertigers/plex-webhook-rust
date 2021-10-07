use bytes::BufMut;
use futures::TryStreamExt;
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tokio::io::AsyncWriteExt;
use tokio::time::timeout;
use warp::http::StatusCode;
use warp::multipart::Part;

use crate::App;

#[derive(Debug, Deserialize, Serialize)]
pub struct Account {
    pub id: u64,
    pub thumb: String,
    pub title: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Server {
    pub title: String,
    pub uuid: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Player {
    pub local: bool,
    pub public_address: String,
    pub title: String,
    pub uuid: String,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Payload {
    pub event: String,
    pub user: bool,
    pub owner: bool,
    #[serde(rename(deserialize = "Account"))]
    pub account: Account,
    #[serde(rename(deserialize = "Server"))]
    pub server: Server,
    #[serde(rename(deserialize = "Player"))]
    pub player: Player,
}

// This is a best effort attempt at firing off the user supplied command
async fn call_command(app: &App, payload: Payload) {
    let path = match tokio::fs::canonicalize(&app.cmd).await {
        Ok(p) => p,
        Err(e) => {
            tracing::error!("cmd {:?}: {}", &app.cmd, e);
            return;
        }
    };

    // This is best effort but we will log some info
    match tokio::process::Command::new(&path)
        .stdin(Stdio::piped())
        .env("PLEX_EVENT", &payload.event)
        .env("PLEX_USER", &payload.account.title)
        .env("PLEX_SERVER", &payload.server.title)
        .env("PLEX_PLAYER", &payload.player.title)
        .spawn()
    {
        Ok(mut child) => {
            let stdin = child.stdin.as_mut().unwrap();
            if let Err(e) = stdin
                .write_all(&serde_json::to_vec(&payload).unwrap())
                .await
            {
                tracing::error!("failed to write to stdin: {}", e);
            }
            match timeout(Duration::from_secs(app.timeout), child.wait()).await {
                Ok(wait) => match wait {
                    Ok(s) => tracing::info!("{:?} -- {}", &path, s),
                    Err(e) => tracing::error!("waiting on {:?} failed: {}", &path, e),
                },
                Err(_) => {
                    child.kill().await.expect("failed to kill child");
                    tracing::error!(
                        "{:?} killed -- failed to execute in {} second(s)",
                        &path,
                        app.timeout
                    )
                }
            }
        }
        Err(e) => tracing::error!("failed to exec {:?}: {}", &path, e),
    }
}

pub async fn handle_webhook(
    form: warp::multipart::FormData,
    app: Arc<App>,
) -> Result<impl warp::Reply, Infallible> {
    let parts: Vec<Part> = match form.try_collect().await {
        Ok(v) => v,
        Err(e) => {
            tracing::error!("form tracing::error: {}", e);
            return Ok(StatusCode::INTERNAL_SERVER_ERROR);
        }
    };

    for p in parts {
        if p.name() != "payload" {
            continue;
        };

        let value = match p
            .stream()
            .try_fold(Vec::new(), |mut vec, data| {
                vec.put(data);
                async move { Ok(vec) }
            })
            .await
        {
            Ok(v) => v,
            Err(e) => {
                tracing::error!("tracing::error reading plex webhook payload: {}", e);
                return Ok(StatusCode::INTERNAL_SERVER_ERROR);
            }
        };

        if let Ok(json) = serde_json::from_slice::<Payload>(&value) {
            tokio::spawn(async move { call_command(&app, json).await });
            return Ok(StatusCode::OK);
        }
    }

    // Didn't find our payload in the FormData
    Ok(StatusCode::BAD_REQUEST)
}
