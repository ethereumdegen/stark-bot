[HyperClaw — Sign Transaction]

A transaction or message needs your signature.

Data:
{data}

**If `action` is `register` or `add_key`:**

This is an EIP-712 typed data signature. Use `sign_typed_data` with the `eip712_data` from the data above.

Then submit:
```
local_rpc(url="http://127.0.0.1:9108/rpc/sign", method="POST", body={
  "tx_id": <tx_id from data above>,
  "signature": "<signature from sign_typed_data result>",
  "eip712_data": <eip712_data from data above>
})
```

**If `action` is `deposit` or `approve_usdc`:**

This is an on-chain transaction. Use `sign_raw_tx` to sign with the fields from the data above (to, data, value, gas, chain_id).

Then submit:
```
local_rpc(url="http://127.0.0.1:9108/rpc/sign", method="POST", body={
  "tx_id": <tx_id from data above>,
  "signed_tx": "<signed_tx hex from sign_raw_tx result>"
})
```

After submitting, call `task_fully_completed` with a brief summary.
