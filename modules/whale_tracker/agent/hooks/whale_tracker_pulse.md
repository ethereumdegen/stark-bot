[Whale Tracker Alert — {timestamp}]

New whale activity detected:
```json
{data}
```

**Your task:**

1. Analyze the signals in the data above.
2. For each significant signal (confidence >= 60):
   - Summarize: WHO moved WHAT, WHERE, and HOW MUCH
   - State the classification (exchange deposit = likely selling, exchange withdrawal = likely accumulating, wallet transfer = repositioning)
   - Include the whale's historical accuracy rate if available
   - State the signal direction (bearish/bullish) and confidence score
3. If multiple signals align (e.g., multiple whales depositing to exchanges), note the convergence — this increases conviction.
4. Cross-reference with memory via `memory_search` for any relevant past patterns from these whales.
5. Store any notable patterns via `kv_store` for future reference.
6. Call `task_fully_completed` with your analysis summary.
