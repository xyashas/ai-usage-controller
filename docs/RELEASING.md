# Releasing

## One-time setup

1. **Updater key.** Run `cd app && npx tauri signer generate -w ~/.tauri/aiuc.key`, then:
   - paste the public key into `app/src-tauri/tauri.updater.conf.json` (`plugins.updater.pubkey`);
   - add repo secrets `TAURI_SIGNING_PRIVATE_KEY` (the file contents) and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD`.
2. **Apple** ($99/yr Developer Program). Export a *Developer ID Application* certificate as .p12, then add these secrets:
   - `APPLE_CERTIFICATE`: base64 of the .p12
   - `APPLE_CERTIFICATE_PASSWORD`
   - `APPLE_SIGNING_IDENTITY`, e.g. `Developer ID Application: Your Name (TEAMID)`
   - `APPLE_ID`, `APPLE_PASSWORD` (an app-specific password) and `APPLE_TEAM_ID` for notarization
3. **Windows code signing.** Without it SmartScreen warns and installs drop. Two options:
   - **Azure Trusted Signing** (cheapest, no hardware token): set `bundle.windows.signCommand` in `tauri.conf.json` to the `trusted-signing-cli` command and add the Azure credentials as secrets.
   - **OV/EV certificate**: set `bundle.windows.certificateThumbprint`, `digestAlgorithm: "sha256"` and `timestampUrl`.
4. **Lemon Squeezy** (merchant of record):
   - Create products named *Pro* (monthly $9, yearly $79) and *Team* (per-seat $29, quantity ≥ 3). The app reads the product or variant name, so any name containing "Team" unlocks Team.
   - Turn on **license keys** for both products, with activation limit 2 for Pro.
   - Set the repo variable `AIUC_LS_STORE_ID` so keys from other stores are rejected.
   - Add a webhook to `https://<api>/v1/webhooks/lemonsqueezy` with the `subscription_*` events, and put the signing secret in the API's `LEMONSQUEEZY_WEBHOOK_SECRET`. For team checkouts, pass `checkout[custom][org_id]=<org id>`.
   - Put the checkout URLs in `ui/.env` (`VITE_CHECKOUT_ANNUAL_URL`, `VITE_CHECKOUT_MONTHLY_URL`) and `web/.env` (see `web/.env.example`).
5. **API.** See the commands at the top of `api/fly.toml`. Any host with a persistent disk works; the database is a single SQLite file.
6. **Website.** Import `web/` into Vercel and set the `NEXT_PUBLIC_*` variables.

## Cutting a release

1. Bump `version` in `app/src-tauri/tauri.conf.json` and `Cargo.toml` (workspace).
2. Tag and push the tag:

   ```bash
   git tag v0.1.0
   ```

   ```bash
   git push origin v0.1.0
   ```

3. The `Release` workflow builds a signed universal `.dmg` and Windows `.exe`/`.msi`, plus `latest.json` for the updater, into a **draft** release. Check it, then publish. Installed apps pick it up within 12 hours.

## Price changes

When a vendor changes prices or plans, edit `core/data/prices.json` or `plans.json`, commit, and push the table to the live API:

```bash
curl -X PUT -H "x-admin-secret: $ADMIN_SECRET" --data @core/data/prices.json https://api.aiusage.dev/v1/admin/tables/prices
```

Apps pick it up within 6 hours. No release needed.
