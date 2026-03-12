//! Execute controller — REST endpoints for crypto instruction execution.

use actix_web::{web, HttpRequest, HttpResponse};
use serde_json::json;

use crate::AppState;
use crate::crypto_executor::CryptoInstruction;

/// POST /api/execute — execute a CryptoInstruction directly.
pub async fn execute_instruction(
    state: web::Data<AppState>,
    req: HttpRequest,
    body: web::Json<CryptoInstruction>,
) -> HttpResponse {
    // Auth check
    if let Err(resp) = super::validate_session(&state, &req) {
        return resp;
    }

    let executor = match &state.crypto_executor {
        Some(e) => e,
        None => return HttpResponse::ServiceUnavailable().json(json!({
            "error": "Crypto executor not available (no wallet configured)"
        })),
    };

    match executor.execute(body.into_inner()).await {
        Ok(result) => HttpResponse::Ok().json(result),
        Err(e) => HttpResponse::BadRequest().json(json!({ "error": e })),
    }
}

/// POST /webhook/starflask — receive Starflask webhook, parse instructions, execute.
pub async fn starflask_webhook(
    state: web::Data<AppState>,
    body: web::Json<serde_json::Value>,
) -> HttpResponse {
    // Verify internal token
    // Starflask webhooks include the session result
    let executor = match &state.crypto_executor {
        Some(e) => e,
        None => return HttpResponse::ServiceUnavailable().json(json!({
            "error": "Crypto executor not available"
        })),
    };

    let instructions = crate::starflask_bridge::parse_session_result(&Some(body.into_inner()));

    if instructions.is_empty() {
        return HttpResponse::Ok().json(json!({
            "status": "no_instructions",
            "message": "No crypto instructions found in webhook payload"
        }));
    }

    let mut results = Vec::new();
    for instruction in instructions {
        match executor.execute(instruction).await {
            Ok(result) => results.push(result),
            Err(e) => results.push(crate::crypto_executor::ExecutionResult {
                success: false,
                data: json!({ "error": e }),
            }),
        }
    }

    HttpResponse::Ok().json(json!({
        "status": "executed",
        "results": results,
    }))
}
