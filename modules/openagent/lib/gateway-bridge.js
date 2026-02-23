/**
 * Bridge between XMTP messages and stark-bot's gateway chat API.
 * Maps XMTP sender addresses to persistent session IDs for conversation continuity.
 */

const SELF_URL = Deno.env.get("STARKBOT_SELF_URL") || "http://localhost:3000";
const GATEWAY_TOKEN = Deno.env.get("OPENAGENT_GATEWAY_TOKEN") || Deno.env.get("STARKBOT_INTERNAL_TOKEN");

// Persistent session map: xmtp_address → session_id
const sessions = new Map();

/**
 * Send a user message through the gateway and get the AI response.
 * @param {string} senderAddress — XMTP sender address (used as session key)
 * @param {string} message — message text
 * @param {string} [userName] — optional display name
 * @returns {Promise<string>} AI response text
 */
export async function chat(senderAddress, message, userName) {
  const sessionId = sessions.get(senderAddress) || undefined;

  const res = await fetch(`${SELF_URL}/api/gateway/chat`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Authorization: `Bearer ${GATEWAY_TOKEN}`,
    },
    body: JSON.stringify({
      message,
      session_id: sessionId ? String(sessionId) : undefined,
      user_name: userName || `xmtp:${senderAddress.slice(0, 8)}`,
    }),
  });

  if (!res.ok) {
    const text = await res.text();
    console.error(`[gateway-bridge] Chat failed (${res.status}): ${text}`);
    throw new Error(`Gateway chat failed: ${res.status}`);
  }

  const data = await res.json();

  if (data.session_id) {
    sessions.set(senderAddress, String(data.session_id));
  }

  return data.response || "(no response)";
}
