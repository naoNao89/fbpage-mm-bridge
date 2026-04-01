# Facebook Long-Lived Access Token Guide

## Overview

Use short-lived token from Graph API Explorer to create long-lived tokens:
- **User tokens**: ~60 days
- **Page tokens**: Never expires (recommended for production)

## Token Types

| Token Type | Expiration | Use Case |
|------------|------------|----------|
| Short-lived User | ~2 hours | Initial login flow |
| Long-lived User | ~60 days | Extended server operations |
| Long-lived Page | **Never** | Production messaging |

## Getting Long-Lived Tokens

### Prerequisites

1. Facebook App ID: `YOUR_APP_ID`
2. Facebook App Secret: `YOUR_APP_SECRET`
3. Facebook User Access Token (from Graph API Explorer - **this is short-lived**)

### Step 1: Get Short-Lived User Token from Graph API Explorer

The short-lived token is obtained from Graph API Explorer and expires in ~2 hours.

1. Go to [Graph API Explorer](https://developers.facebook.com/tools/explorer/)
2. Select your app
3. Click "Generate Access Token"
4. Grant `pages_messaging` permission
5. Copy the token (starts with `EAA...`)

**This token expires quickly - exchange it for long-lived immediately.**

### Step 2: Exchange Short-Lived to Long-Lived User Token

Exchange your short-lived token for a long-lived user token that lasts ~60 days.

```bash
curl "https://graph.facebook.com/v24.0/oauth/access_token?\
  grant_type=fb_exchange_token&\
  client_id=YOUR_APP_ID&\
  client_secret=YOUR_APP_SECRET&\
  fb_exchange_token=YOUR_SHORT_LIVED_TOKEN"
```

Response:
```json
{
  "access_token": "LONG_LIVED_USER_TOKEN...",
  "token_type": "bearer",
  "expires_in": 5183944
}
```

### Step 3: Get Long-Lived Page Access Token (Recommended)

Use the long-lived user token to get a page access token that **never expires**.

```bash
curl "https://graph.facebook.com/v24.0/YOUR_USER_ID/accounts?\
  access_token=LONG_LIVED_USER_TOKEN"
```

Response:
```json
{
  "data": [{
    "access_token": "LONG_LIVED_PAGE_TOKEN...",
    "name": "Your Page Name",
    "id": "PAGE_ID",
    "tasks": ["MODERATE", "MESSAGING", ...]
  }]
}
```

**Use this page token in production - it never expires.**

## Token Renewal

### Long-Lived User Token (60 days)

Long-lived user tokens need to be refreshed before they expire. Since this is a server-side service without user interaction:

1. Store the long-lived user token securely
2. **Schedule a monthly refresh job** to exchange it for a new one
3. Use the refreshed token to regenerate page tokens if needed

```bash
# Refresh command (same as Step 2)
curl "https://graph.facebook.com/v24.0/oauth/access_token?\
  grant_type=fb_exchange_token&\
  client_id=YOUR_APP_ID&\
  client_secret=YOUR_APP_SECRET&\
  fb_exchange_token=EXISTING_LONG_LIVED_TOKEN"
```

### Long-Lived Page Token (Never expires)

Page tokens **do not need renewal** - use them for all production messaging operations.

Page tokens only expire if:
- You change your page password
- You remove the app from Page settings
- Facebook revokes due to policy violation

Recommended: Use Page tokens for all messaging operations.

## Configuration

Set in `.env`:

```bash
FACEBOOK_PAGE_ID=YOUR_PAGE_ID
FACEBOOK_PAGE_ACCESS_TOKEN=LONG_LIVED_PAGE_TOKEN...
FACEBOOK_APP_ID=YOUR_APP_ID
FACEBOOK_APP_SECRET=YOUR_APP_SECRET
```

## Verification

Test your token:

```bash
curl "https://graph.facebook.com/v24.0/me/accounts?access_token=PAGE_TOKEN"
```

Should return your page info.

## Common Errors

| Error | Solution |
|-------|----------|
| `Invalid OAuth access token` | Token expired or malformed |
| `Token doesn't match` | Using user token instead of page token |
| `Permission denied` | Need `pages_messaging` permission |

## API Reference

- [Get Long-Lived Access Tokens (Facebook Docs)](https://developers.facebook.com/docs/facebook-login/guides/access-tokens/get-long-lived)
- [Get Long-Lived Page Tokens](https://developers.facebook.com/docs/pages/manage-assets/)
- [Graph API Explorer](https://developers.facebook.com/tools/explorer/)
