use actix_multipart::Multipart;
use actix_web::{web, HttpRequest, HttpResponse, Responder};
use futures_util::StreamExt;
use serde::Serialize;

use crate::AppState;

#[derive(Serialize)]
struct TranscribeResponse {
    success: bool,
    text: Option<String>,
    error: Option<String>,
}

fn validate_session_from_request(
    state: &web::Data<AppState>,
    req: &HttpRequest,
) -> Result<(), HttpResponse> {
    let token = req
        .headers()
        .get("Authorization")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.trim_start_matches("Bearer ").to_string());

    let token = match token {
        Some(t) => t,
        None => {
            return Err(HttpResponse::Unauthorized().json(TranscribeResponse {
                success: false,
                text: None,
                error: Some("No authorization token provided".to_string()),
            }));
        }
    };

    match state.db.validate_session(&token) {
        Ok(Some(_)) => Ok(()),
        Ok(None) => Err(HttpResponse::Unauthorized().json(TranscribeResponse {
            success: false,
            text: None,
            error: Some("Invalid or expired session".to_string()),
        })),
        Err(e) => {
            log::error!("Failed to validate session: {}", e);
            Err(HttpResponse::InternalServerError().json(TranscribeResponse {
                success: false,
                text: None,
                error: Some("Internal server error".to_string()),
            }))
        }
    }
}

const MAX_AUDIO_SIZE: usize = 25 * 1024 * 1024; // 25MB

async fn transcribe(
    state: web::Data<AppState>,
    req: HttpRequest,
    mut payload: Multipart,
) -> impl Responder {
    if let Err(resp) = validate_session_from_request(&state, &req) {
        return resp;
    }

    // Read audio data from multipart
    let mut audio_data: Vec<u8> = Vec::new();
    let mut content_type: Option<String> = None;
    let mut filename: Option<String> = None;

    while let Some(item) = payload.next().await {
        match item {
            Ok(mut field) => {
                let field_name = field.name().to_string();
                if field_name != "audio" {
                    continue;
                }

                content_type = field.content_type().map(|ct| ct.to_string());
                filename = field
                    .content_disposition()
                    .get_filename()
                    .map(|s| s.to_string());

                while let Some(chunk) = field.next().await {
                    match chunk {
                        Ok(data) => {
                            audio_data.extend_from_slice(&data);
                            if audio_data.len() > MAX_AUDIO_SIZE {
                                return HttpResponse::PayloadTooLarge().json(TranscribeResponse {
                                    success: false,
                                    text: None,
                                    error: Some("Audio file exceeds 25MB limit".to_string()),
                                });
                            }
                        }
                        Err(e) => {
                            return HttpResponse::BadRequest().json(TranscribeResponse {
                                success: false,
                                text: None,
                                error: Some(format!("Failed to read audio data: {}", e)),
                            });
                        }
                    }
                }
            }
            Err(e) => {
                return HttpResponse::BadRequest().json(TranscribeResponse {
                    success: false,
                    text: None,
                    error: Some(format!("Failed to process multipart: {}", e)),
                });
            }
        }
    }

    if audio_data.is_empty() {
        return HttpResponse::BadRequest().json(TranscribeResponse {
            success: false,
            text: None,
            error: Some("No audio data provided".to_string()),
        });
    }

    // Determine whisper server URL from bot_config.ron (default if not configured)
    let whisper_url = crate::models::BotConfig::load()
        .services.whisper_server_url
        .unwrap_or_else(|| crate::models::DEFAULT_WHISPER_SERVER_URL.to_string());
    let url = format!("{}/transcribe", whisper_url.trim_end_matches('/'));

    // Build multipart form to forward to whisper server
    let fname = filename.unwrap_or_else(|| "audio.webm".to_string());
    let mime = content_type.unwrap_or_else(|| "audio/webm".to_string());

    let audio_part = reqwest::multipart::Part::bytes(audio_data)
        .file_name(fname)
        .mime_str(&mime)
        .unwrap_or_else(|_| {
            reqwest::multipart::Part::bytes(vec![])
                .file_name("audio.webm")
        });

    let form = reqwest::multipart::Form::new()
        .part("audio", audio_part)
        .text("language", "en");

    // Forward to whisper server
    let client = reqwest::Client::new();
    let result = client
        .post(&url)
        .multipart(form)
        .timeout(std::time::Duration::from_secs(30))
        .send()
        .await;

    match result {
        Ok(resp) => {
            if !resp.status().is_success() {
                let status = resp.status();
                let body = resp.text().await.unwrap_or_default();
                log::error!("Whisper server returned {}: {}", status, body);
                return HttpResponse::BadGateway().json(TranscribeResponse {
                    success: false,
                    text: None,
                    error: Some(format!("Whisper server error: {} {}", status, body)),
                });
            }

            // Parse whisper server response: { "text": "..." }
            match resp.json::<serde_json::Value>().await {
                Ok(json) => {
                    let text = json
                        .get("text")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();

                    HttpResponse::Ok().json(TranscribeResponse {
                        success: true,
                        text: Some(text),
                        error: None,
                    })
                }
                Err(e) => {
                    log::error!("Failed to parse whisper response: {}", e);
                    HttpResponse::BadGateway().json(TranscribeResponse {
                        success: false,
                        text: None,
                        error: Some("Invalid response from whisper server".to_string()),
                    })
                }
            }
        }
        Err(e) => {
            log::error!("Failed to reach whisper server at {}: {}", url, e);
            HttpResponse::BadGateway().json(TranscribeResponse {
                success: false,
                text: None,
                error: Some(format!("Cannot reach whisper server: {}", e)),
            })
        }
    }
}

pub fn config(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/api/transcribe")
            .route("", web::post().to(transcribe)),
    );
}
