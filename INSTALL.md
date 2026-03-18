# ClaudeCodeClaw — Install

One script. Requires Docker, npm, and a Linux server.

```bash
curl -fsSL https://raw.githubusercontent.com/rob-linton/claudecodeclaw/main/scripts/install.sh | bash -s /opt/claw
```

Or download and run manually:

```bash
wget https://raw.githubusercontent.com/rob-linton/claudecodeclaw/main/scripts/install.sh
chmod +x install.sh
./install.sh /opt/claw
```

The script will:
1. Ask for your server IP and admin password
2. Generate all config files (docker-compose.yml, Caddyfile, .env)
3. Launch Claude Code for interactive OAuth login
4. Pull pre-built Docker images and start 5 services
5. Print the dashboard URL and extract the TLS CA cert

After install, give `claw-ca.crt` to your team to avoid browser TLS warnings.

## After install

```bash
cd /opt/claw

# View logs
docker compose --env-file .env logs -f worker

# Restart
docker compose --env-file .env restart

# Stop
docker compose --env-file .env down

# Re-authenticate Claude (token expired)
claude    # complete login, then /exit
docker compose --env-file .env restart worker
```
