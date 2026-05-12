# Reality Graph Console

Minimal Next.js admin console for the Reality Graph API.

```bash
npm install
npm run dev
```

The console proxies API calls through `/api/rg/*` to `RG_API_URL`, falling back to
`NEXT_PUBLIC_RG_API_URL` and then `http://127.0.0.1:8080`.
