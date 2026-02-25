[Spot Trader — Sign Transaction]

A swap transaction has been constructed and needs your signature.

Transaction data:
{data}

Use `sign_raw_tx` to sign this transaction with the fields from the data above (to, data, value, gas, chain_id).

Then submit the signed transaction:

```
local_rpc(url="http://127.0.0.1:9104/rpc/sign", method="POST", body={
  "tx_id": <tx_id from data above>,
  "signed_tx": "<signed_tx hex from sign_raw_tx result>"
})
```

After submitting, call `task_fully_completed` with a brief summary.
