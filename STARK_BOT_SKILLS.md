# SKILL Stark-Bot: Skill Development Guide
**Expand the capabilities of your Starknet agent.**

Skills are modular plugins that allow Stark-Bot to interact with new protocols, external APIs, or custom on-chain logic.

---

## FOLDER 1. Skill Structure
Each skill lives in its own directory within `/skills`:
```text
/skills/your-skill-name/
├── index.ts        # Main logic
├── manifest.json   # Metadata & permissions
└── README.md       # Documentation for your skill
```

## CONFIG 2. The Manifest (`manifest.json`)
Define what your skill needs to function:
```json
{
  "name": "my-custom-skill",
  "version": "1.0.0",
  "description": "Interacts with protocol X",
  "permissions": ["starknet_read", "starknet_write", "notification_send"],
  "config": {
    "apiKey": "string",
    "threshold": "number"
  }
}
```

## LOGIC 3. Implementing Logic (`index.ts`)
Your skill must export an `init` and an `execute` function:
```typescript
import { StarkNetProvider, Logger } from '../../src/core';

export async function execute(context: any, params: any) {
  Logger.info("Executing custom skill...");
  const balance = await StarkNetProvider.getBalance(context.userAddress);
  // Your custom logic here...
}
```

## START 4. Activation
To enable your skill:
1. Place the folder in `/skills`.
2. Add the skill name to your `config/settings.json`:
   ```json
   "enabled_skills": ["my-custom-skill"]
   ```
3. Restart the bot or run `npm run reload:skills`.

---
**Need help?** Check the `FUTURE_UPGRADES.md` for upcoming API changes.

