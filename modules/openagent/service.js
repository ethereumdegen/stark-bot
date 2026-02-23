/**
 * OpenAgent XMTP Module for stark-bot
 *
 * Runs on Deno — no npm install needed. Connects to XMTP via a proxy signer
 * (no private key in this process). Exposes RPC endpoints for the tool registry
 * and forwards inbound XMTP messages to the gateway chat API.
 *
 * Usage: deno run --allow-net --allow-env --allow-read --allow-write service.js
 */

import { Client } from "npm:@xmtp/node-sdk@^1.0.2";
import { createProxySigner } from "./lib/signer-proxy.js";
import * as gatewayBridge from "./lib/gateway-bridge.js";

const PORT = parseInt(Deno.env.get("MODULE_PORT") || Deno.env.get("OPENAGENT_PORT") || "9102", 10);
const HOME = Deno.env.get("HOME") || "/tmp";
const DATA_DIR = Deno.env.get("OPENAGENT_DATA_DIR") || `${HOME}/.starkbot/openagent`;
const XMTP_ENV = Deno.env.get("XMTP_ENV") || "production";

let xmtpClient = null;
let clientAddress = null;

// ── XMTP Client Setup ──────────────────────────────────────────────────

async function initXmtpClient() {
  console.log("[openagent] Initializing XMTP client...");

  const signer = await createProxySigner();
  clientAddress = signer.getIdentifier().identifier;

  // Ensure data dir exists for XMTP local DB
  const dbDir = `${DATA_DIR}/xmtp-db`;
  await Deno.mkdir(dbDir, { recursive: true });
  const dbPath = `${dbDir}/${clientAddress}.db3`;

  // Deterministic 32-byte encryption key for XMTP local DB (NOT a signing key)
  const enc = new TextEncoder();
  const hashBuf = await crypto.subtle.digest("SHA-256", enc.encode(`starkbot-xmtp-db-${clientAddress}`));
  const encryptionKey = new Uint8Array(hashBuf);

  xmtpClient = await Client.create(signer, encryptionKey, {
    env: XMTP_ENV,
    dbPath,
  });

  console.log(`[openagent] XMTP client ready — address: ${clientAddress}, env: ${XMTP_ENV}`);
  startMessageListener();
}

// ── Message Listener ────────────────────────────────────────────────────

async function startMessageListener() {
  console.log("[openagent] Starting XMTP message listener...");

  try {
    await xmtpClient.conversations.sync();
    const stream = await xmtpClient.conversations.streamAllMessages();

    for await (const message of stream) {
      try {
        if (message.senderInboxId === xmtpClient.inboxId) continue;
        if (message.contentType?.typeId !== "text" && typeof message.content !== "string") continue;

        const text = typeof message.content === "string" ? message.content : String(message.content);
        const senderAddress = message.senderInboxId;

        console.log(`[openagent] Inbound from ${senderAddress}: ${text.slice(0, 100)}${text.length > 100 ? "..." : ""}`);

        const response = await gatewayBridge.chat(senderAddress, text);

        const conversation = await xmtpClient.conversations.getConversationById(message.conversationId);
        if (conversation) {
          await conversation.send(response);
          console.log(`[openagent] Replied to ${senderAddress}: ${response.slice(0, 100)}${response.length > 100 ? "..." : ""}`);
        }
      } catch (err) {
        console.error("[openagent] Error handling message:", err.message);
      }
    }
  } catch (err) {
    console.error("[openagent] Message listener error:", err.message);
    setTimeout(() => startMessageListener(), 5000);
  }
}

// ── RPC Router ──────────────────────────────────────────────────────────

async function handleRequest(req) {
  const url = new URL(req.url);
  const path = url.pathname;

  // Health check
  if (req.method === "GET" && path === "/rpc/status") {
    return json({
      status: "ok",
      module: "openagent",
      xmtp_connected: xmtpClient !== null,
      address: clientAddress,
      env: XMTP_ENV,
    });
  }

  // All tool endpoints are POST
  if (req.method !== "POST") {
    return json({ error: "Method not allowed" }, 405);
  }

  const body = await req.json().catch(() => ({}));

  if (path === "/rpc/send_message") return await handleSendMessage(body);
  if (path === "/rpc/discover_agents") return await handleDiscoverAgents(body);
  if (path === "/rpc/send_task") return await handleSendTask(body);
  if (path === "/rpc/conversations") return await handleConversations(body);

  return json({ error: "Not found" }, 404);
}

// ── Tool Handlers ───────────────────────────────────────────────────────

async function handleSendMessage({ recipient, message }) {
  if (!xmtpClient) return json({ error: "XMTP client not initialized" }, 503);
  if (!recipient || !message) return json({ error: "recipient and message are required" }, 400);

  try {
    const conversation = await xmtpClient.conversations.newDm(recipient);
    await conversation.send(message);
    return json({ success: true, conversation_id: conversation.id, recipient });
  } catch (err) {
    console.error("[openagent] send_message error:", err.message);
    return json({ error: err.message }, 500);
  }
}

async function handleDiscoverAgents({ query, limit = 10 }) {
  if (!query) return json({ error: "query is required" }, 400);

  try {
    const discoveryUrl = Deno.env.get("OPENAGENT_DISCOVER_URL") || "https://openagentmarket.xyz/api/agents";
    const res = await fetch(`${discoveryUrl}?q=${encodeURIComponent(query)}&limit=${limit}`);
    if (!res.ok) return json({ error: `Discovery API returned ${res.status}` }, res.status);
    const data = await res.json();
    return json({ success: true, agents: data.agents || data });
  } catch (err) {
    console.error("[openagent] discover_agents error:", err.message);
    return json({ error: err.message }, 500);
  }
}

async function handleSendTask({ agent_address, method, params, timeout_ms = 30000 }) {
  if (!xmtpClient) return json({ error: "XMTP client not initialized" }, 503);
  if (!agent_address || !method) return json({ error: "agent_address and method are required" }, 400);

  try {
    const rpcRequest = {
      jsonrpc: "2.0",
      id: Date.now().toString(),
      method,
      params: typeof params === "string" ? JSON.parse(params) : params,
    };

    const conversation = await xmtpClient.conversations.newDm(agent_address);
    await conversation.send(JSON.stringify(rpcRequest));

    const response = await waitForResponse(conversation, rpcRequest.id, timeout_ms);
    return json({ success: true, result: response });
  } catch (err) {
    console.error("[openagent] send_task error:", err.message);
    return json({ error: err.message }, 500);
  }
}

async function waitForResponse(conversation, requestId, timeoutMs) {
  return new Promise((resolve, reject) => {
    const timeout = setTimeout(() => reject(new Error(`Task timed out after ${timeoutMs}ms`)), timeoutMs);

    (async () => {
      try {
        const stream = await conversation.stream();
        for await (const msg of stream) {
          if (msg.senderInboxId === xmtpClient.inboxId) continue;
          const text = typeof msg.content === "string" ? msg.content : String(msg.content);
          try {
            const parsed = JSON.parse(text);
            if (parsed.id === requestId) {
              clearTimeout(timeout);
              stream.return?.();
              resolve(parsed.result !== undefined ? parsed.result : parsed);
              return;
            }
          } catch {
            clearTimeout(timeout);
            stream.return?.();
            resolve(text);
            return;
          }
        }
      } catch (err) {
        clearTimeout(timeout);
        reject(err);
      }
    })();
  });
}

async function handleConversations({ limit = 20 }) {
  if (!xmtpClient) return json({ error: "XMTP client not initialized" }, 503);

  try {
    await xmtpClient.conversations.sync();
    const allConversations = await xmtpClient.conversations.list();
    const recent = allConversations.slice(0, limit);

    const conversations = await Promise.all(
      recent.map(async (conv) => {
        try {
          await conv.sync();
          const messages = await conv.messages({ limit: 1n });
          const lastMessage = messages[0];
          return {
            id: conv.id,
            peer_address: conv.peerInboxId,
            last_message: lastMessage
              ? {
                  content: typeof lastMessage.content === "string"
                    ? lastMessage.content.slice(0, 200)
                    : String(lastMessage.content).slice(0, 200),
                  sent_at: lastMessage.sentAtNs
                    ? new Date(Number(lastMessage.sentAtNs) / 1_000_000).toISOString()
                    : null,
                }
              : null,
          };
        } catch {
          return { id: conv.id, peer_address: conv.peerInboxId, last_message: null };
        }
      })
    );

    return json({ success: true, conversations });
  } catch (err) {
    console.error("[openagent] conversations error:", err.message);
    return json({ error: err.message }, 500);
  }
}

// ── Helpers ─────────────────────────────────────────────────────────────

function json(data, status = 200) {
  return new Response(JSON.stringify(data), {
    status,
    headers: { "Content-Type": "application/json" },
  });
}

// ── Start ───────────────────────────────────────────────────────────────

console.log(`[openagent] RPC server listening on port ${PORT}`);

Deno.serve({ port: PORT }, handleRequest);

initXmtpClient().catch((err) => {
  console.error("[openagent] Failed to initialize XMTP client:", err.message);
  console.error("[openagent] RPC server is running but XMTP features are unavailable");
});
