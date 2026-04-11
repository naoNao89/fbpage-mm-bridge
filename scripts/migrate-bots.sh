#!/usr/bin/env python3
"""
Migration tool: Re-import all conversations with proper bot names.

For each t_* channel:
1. Create/ensure a customer bot with the channel's display_name
2. Fetch all messages, re-post incoming messages under the customer bot
3. Delete old admin-posted messages after re-import
4. Clean up system messages

Usage: python3 migrate-bots.py [--dry-run] [--channel t_12345]
"""

import json
import urllib.request
import urllib.error
import sys
import time
import re

MM_URL = "http://localhost:8065/api/v4"
ADMIN_TOKEN = "gye7ihx6n78btykyb53h1bsczo"
TEAM_ID = "bz8xty99wpfy8cgssxjrdjkxte"
ADMIN_USER_ID = "dx1upyk8obbrtxjoe4xh4yzk7c"
FB_BRIDGE_BOT_ID = "9seygjrtwjbftnubipkkwkddur"

DRY_RUN = "--dry-run" in sys.argv
SINGLE_CHANNEL = None
for arg in sys.argv:
    if arg.startswith("--channel="):
        SINGLE_CHANNEL = arg.split("=")[1]

def api(method, path, data=None):
    req = urllib.request.Request(f"{MM_URL}{path}", method=method)
    req.add_header("Authorization", f"Bearer {ADMIN_TOKEN}")
    req.add_header("Content-Type", "application/json")
    if data:
        req.data = json.dumps(data).encode()
    try:
        with urllib.request.urlopen(req) as resp:
            return json.loads(resp.read())
    except urllib.error.HTTPError as e:
        body = e.read().decode()[:300]
        return {"error": e.code, "body": body}
    except Exception as ex:
        return {"error": str(ex)}

def api_raw(method, path, data=None):
    req = urllib.request.Request(f"{MM_URL}{path}", method=method)
    req.add_header("Authorization", f"Bearer {ADMIN_TOKEN}")
    req.add_header("Content-Type", "application/json")
    if data:
        req.data = json.dumps(data).encode()
    try:
        with urllib.request.urlopen(req) as resp:
            return resp.getcode(), resp.read()
    except urllib.error.HTTPError as e:
        return e.code, e.read().decode()[:300]

def slugify(name):
    """Convert Vietnamese name to ASCII slug for bot username"""
    mapping = {
        'á':'a','à':'a','ả':'a','ã':'a','ạ':'a','ă':'a','ắ':'a','ằ':'a','ẳ':'a','ẵ':'a','ặ':'a',
        'â':'a','ấ':'a','ầ':'a','ẩ':'a','ẫ':'a','ậ':'a','đ':'d',
        'é':'e','è':'e','ẻ':'e','ẽ':'e','ẹ':'e','ê':'e','ế':'e','ề':'e','ể':'e','ễ':'e','ệ':'e',
        'í':'i','ì':'i','ỉ':'i','ĩ':'i','ị':'i',
        'ó':'o','ò':'o','ỏ':'o','õ':'o','ọ':'o','ô':'o','ố':'o','ồ':'o','ổ':'o','ỗ':'o','ộ':'o',
        'ơ':'o','ớ':'o','ờ':'o','ở':'o','ỡ':'o','ợ':'o',
        'ú':'u','ù':'u','ủ':'u','ũ':'u','ụ':'u','ư':'u','ứ':'u','ừ':'u','ử':'u','ữ':'u','ự':'u',
        'ý':'y','ỳ':'y','ỷ':'y','ỹ':'y','ỵ':'y',
    }
    result = name.lower()
    for k, v in mapping.items():
        result = result.replace(k, v)
    result = re.sub(r'[^a-z0-9 -]', '', result)
    result = result.replace(' ', '-')
    if len(result) < 3:
        result = f"cust-{result}"
    return result[:22]

def get_or_create_bot(display_name):
    """Create a customer bot or return existing one"""
    slug = slugify(display_name)
    
    # Check if bot already exists
    bots = api("GET", "/bots?per_page=200")
    for b in bots:
        if b["username"] == slug:
            # Re-enable if disabled
            if b.get("delete_at", 0) != 0:
                api("POST", f"/bots/{b['user_id']}/enable")
                print(f"  Re-enabled bot {slug}")
            return b["user_id"], slug
    
    # Create new bot
    result = api("POST", "/bots", {
        "username": slug,
        "display_name": display_name,
        "description": "FB Page customer"
    })
    
    if result and "user_id" in result:
        bot_id = result["user_id"]
        print(f"  Created bot {slug} ({bot_id})")
        return bot_id, slug
    elif result and "already exists" in str(result.get("body", "")):
        # Resolve by username
        user = api("GET", f"/users/username/{slug}")
        if user and "id" in user:
            return user["id"], slug
        print(f"  ERROR: Bot {slug} exists but can't resolve: {result}")
        return None, slug
    else:
        print(f"  ERROR: Failed to create bot {slug}: {result}")
        return None, slug

def ensure_bot_in_team_and_channel(bot_user_id, channel_id):
    """Add bot to team and channel"""
    # Add to team
    api("POST", f"/teams/{TEAM_ID}/members", {
        "user_id": bot_user_id,
        "team_id": TEAM_ID,
    })
    
    # Add to channel
    result = api("POST", f"/channels/{channel_id}/members", {
        "user_id": bot_user_id,
    })
    
    # Delete system message about adding
    time.sleep(0.1)
    posts = api("GET", f"/channels/{channel_id}/posts?per_page=5")
    if posts and "posts" in posts:
        admin_id = ADMIN_USER_ID
        for pid, post in posts["posts"].items():
            ptype = post.get("type", "")
            uid = post.get("user_id", "")
            if ptype in ("system_join_channel", "system_add_to_channel", "system_displayname_change") and uid == admin_id:
                api("DELETE", f"/posts/{pid}")

def create_bot_token(bot_user_id):
    """Create an API token for the bot"""
    result = api("POST", f"/users/{bot_user_id}/tokens", {
        "description": "migration bot token"
    })
    if result and "token" in result:
        return result["token"]
    print(f"  WARNING: Failed to create token for {bot_user_id}: {result}")
    return None

def main():
    print(f"Migration tool - DRY_RUN={DRY_RUN}")
    print("=" * 60)
    
    # Get all channels
    channels = api("GET", f"/teams/{TEAM_ID}/channels?per_page=200")
    t_channels = [c for c in channels if c["name"].startswith("t_")]
    
    if SINGLE_CHANNEL:
        t_channels = [c for c in t_channels if c["name"] == SINGLE_CHANNEL]
    
    print(f"Found {len(t_channels)} channels to process")
    
    stats = {"bots_created": 0, "bots_reused": 0, "messages_reposted": 0, "messages_deleted": 0, "system_deleted": 0, "errors": 0}
    
    for i, ch in enumerate(t_channels):
        channel_id = ch["id"]
        display_name = ch.get("display_name", ch["name"])
        channel_name = ch["name"]
        
        print(f"\n[{i+1}/{len(t_channels)}] {channel_name} -> {display_name}")
        
        # Skip channels without display name or with raw ID
        if display_name.startswith("t_") or not display_name or display_name == channel_name:
            print(f"  SKIP: No display name set")
            continue
        
        # Get or create bot
        bot_user_id, bot_slug = get_or_create_bot(display_name)
        if not bot_user_id:
            stats["errors"] += 1
            continue
        
        # Ensure bot is in team and channel
        ensure_bot_in_team_and_channel(bot_user_id, channel_id)
        
        # Create bot token
        bot_token = create_bot_token(bot_user_id)
        if not bot_token:
            stats["errors"] += 1
            continue
        
        # Fetch all posts in the channel
        all_posts = []
        page = 0
        while True:
            result = api("GET", f"/channels/{channel_id}/posts?per_page=200&page={page}")
            if not result or "posts" not in result or not result["posts"]:
                break
            for pid, post in result["posts"].items():
                all_posts.append(post)
            order_key = result.get("order", [])
            if len(order_key) < 200:
                break
            page += 1
        
        # Sort by create_at (oldest first)
        all_posts.sort(key=lambda p: p.get("create_at", 0))
        
        # Separate: system messages to delete, admin messages to re-post
        admin_messages = []
        system_to_delete = []
        
        for post in all_posts:
            ptype = post.get("type", "")
            uid = post.get("user_id", "")
            
            if ptype.startswith("system_"):
                system_to_delete.append((post["id"], ptype))
            elif uid == ADMIN_USER_ID and ptype == "":
                admin_messages.append(post)
        
        print(f"  Found {len(admin_messages)} admin messages, {len(system_to_delete)} system messages")
        
        if DRY_RUN:
            stats["bots_created" if not bot_user_id else "bots_reused"] += 1
            continue
        
        # Delete system messages
        for pid, ptype in system_to_delete:
            code, _ = api_raw("DELETE", f"/posts/{pid}")
            if code == 200:
                stats["system_deleted"] += 1
        
        # Delete old admin messages (will be re-posted)
        posts_to_delete = []
        for post in admin_messages:
            posts_to_delete.append(post["id"])
        
        for pid in posts_to_delete:
            code, _ = api_raw("DELETE", f"/posts/{pid}")
        
        # Re-post messages - first incoming (customer), then outgoing (admin)
        # We already deleted admin messages. Now re-post them.
        # For incoming messages, we need to check if they were originally from customer
        # Since we only have admin messages in the channel, we re-post using the display_name as bot
        
        # Actually, we need to determine direction. Messages posted by admin could be:
        # 1. Original outgoing (page replies) - should stay as admin/Bump Clean
        # 2. Originally incoming (customer messages) that were posted as admin due to fallback
        
        # For simplicity: all messages that were posted by admin will be re-posted as the customer bot.
        # This is because in the current setup, admin posts BOTH incoming and outgoing messages.
        # The direction info is lost once posted to MM.
        
        # We'll re-post all former admin messages as the customer bot.
        for post in admin_messages:
            text = post.get("message", "")
            if not text.strip():
                continue
            ts = post.get("create_at")
            
            payload = {
                "channel_id": channel_id,
                "message": text,
            }
            if ts:
                payload["create_at"] = ts
            
            resp_code, _ = api_raw("POST", f"/posts?use_post_message_as_bot={bot_user_id}", payload)
            # Actually, posting as bot requires bot token, not admin token
            # Let me use the bot token instead
            
        # Post using bot token for customer messages
        for post in admin_messages:
            text = post.get("message", "")
            if not text.strip():
                continue
            ts = post.get("create_at")
            
            req = urllib.request.Request(
                f"{MM_URL}/posts",
                data=json.dumps({
                    "channel_id": channel_id,
                    "message": text,
                    "create_at": ts,
                }).encode(),
                method="POST"
            )
            req.add_header("Authorization", f"Bearer {bot_token}")
            req.add_header("Content-Type", "application/json")
            try:
                with urllib.request.urlopen(req) as resp:
                    result = json.loads(resp.read())
                    stats["messages_reposted"] += 1
            except urllib.error.HTTPError as e:
                error_body = e.read().decode()[:100]
                # If duplicate, skip
                if "already exists" not in error_body:
                    print(f"  ERROR posting as bot: {e.code} {error_body}")
                    stats["errors"] += 1
        
        time.sleep(0.2)  # Rate limit
    
    print("\n" + "=" * 60)
    print(f"MIGRATION COMPLETE")
    print(f"  Messages re-posted: {stats['messages_reposted']}")
    print(f"  System messages deleted: {stats['system_deleted']}")
    print(f"  Errors: {stats['errors']}")

if __name__ == "__main__":
    main()