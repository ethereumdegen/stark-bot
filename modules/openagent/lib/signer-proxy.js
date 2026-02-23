/**
 * XMTP-compatible signer that proxies all signing through stark-bot's
 * internal wallet API. No private key ever crosses process boundaries.
 */

const SELF_URL = Deno.env.get("STARKBOT_SELF_URL") || "http://localhost:3000";
const TOKEN = Deno.env.get("STARKBOT_INTERNAL_TOKEN");

if (!TOKEN) {
  console.error("[signer-proxy] STARKBOT_INTERNAL_TOKEN not set — signing will fail");
}

/** Fetch the wallet address from the backend */
export async function getAddress() {
  const res = await fetch(`${SELF_URL}/api/internal/wallet/address`, {
    headers: { Authorization: `Bearer ${TOKEN}` },
  });
  if (!res.ok) {
    throw new Error(`Failed to get address: ${res.status} ${await res.text()}`);
  }
  const data = await res.json();
  return data.address;
}

/** Convert hex string to Uint8Array */
function hexToBytes(hex) {
  const clean = hex.replace(/^0x/, "");
  const bytes = new Uint8Array(clean.length / 2);
  for (let i = 0; i < bytes.length; i++) {
    bytes[i] = parseInt(clean.substr(i * 2, 2), 16);
  }
  return bytes;
}

/** Convert Uint8Array to hex string */
function bytesToHex(bytes) {
  return Array.from(bytes).map((b) => b.toString(16).padStart(2, "0")).join("");
}

/**
 * Sign arbitrary bytes via the backend wallet provider.
 * @param {Uint8Array|string} message — bytes or utf8 string to sign
 * @returns {Promise<Uint8Array>} raw 65-byte signature
 */
export async function signMessage(message) {
  let body;
  if (message instanceof Uint8Array) {
    body = { message: "0x" + bytesToHex(message), encoding: "hex" };
  } else {
    body = { message: String(message), encoding: "utf8" };
  }

  const res = await fetch(`${SELF_URL}/api/internal/wallet/sign-message`, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      Authorization: `Bearer ${TOKEN}`,
    },
    body: JSON.stringify(body),
  });

  if (!res.ok) {
    throw new Error(`sign-message failed: ${res.status} ${await res.text()}`);
  }

  const data = await res.json();
  if (!data.success) {
    throw new Error(`sign-message error: ${data.error}`);
  }

  return hexToBytes(data.signature);
}

/**
 * Create an XMTP-compatible signer object.
 * Satisfies the interface expected by @xmtp/node-sdk:
 *   { getIdentifier, signMessage, type }
 */
export async function createProxySigner() {
  const address = await getAddress();
  console.log(`[signer-proxy] Wallet address: ${address}`);

  return {
    type: "EOA",
    getIdentifier: () => ({
      identifier: address.toLowerCase(),
      identifierKind: "Eoa",
    }),
    signMessage: async (msg) => await signMessage(msg),
  };
}
