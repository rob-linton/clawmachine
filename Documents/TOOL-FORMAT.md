# Tool Package Format Specification

Version: 1.0

## Overview

A tool package is a ZIP file containing a TOOL.json definition, a manifest, and optionally other files. Tools are CLI programs (e.g., az-cli, aws-cli, jq) installed into Docker sandbox images on demand during job execution.

## ZIP Structure

```
my-tool.zip
├── TOOL.json          # Required: tool definition (install, check, auth)
└── manifest.json      # Recommended: package metadata
```

### Root Directory Stripping

If all files share a common root directory prefix (e.g., `my-tool/TOOL.json`), it is automatically stripped on import.

## TOOL.json

The tool definition file. Specifies how to install, verify, and authenticate the tool.

```json
{
  "install_commands": "apt-get update\napt-get install -y jq\nrm -rf /var/lib/apt/lists/*",
  "check_command": "jq --version",
  "env_vars": [
    {
      "key": "AZURE_CLIENT_ID",
      "description": "Service principal app ID",
      "required": true
    },
    {
      "key": "AZURE_CLIENT_SECRET",
      "description": "Service principal secret",
      "required": true
    }
  ],
  "auth_script": "az login --service-principal -u $AZURE_CLIENT_ID -p $AZURE_CLIENT_SECRET --tenant $AZURE_TENANT_ID"
}
```

### Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `install_commands` | string | Yes | Shell commands to install the tool (targets Docker/Debian). Multiline: each line joined with ` && ` for Dockerfile `RUN` directives. |
| `check_command` | string | Yes | Command to verify installation. Exit code 0 = installed. Example: `jq --version` |
| `env_vars` | object[] | No | Environment variables the tool needs at runtime |
| `auth_script` | string | No | Shell commands run before job execution to authenticate the tool |

### env_vars Object

| Field | Type | Required | Default | Description |
|-------|------|----------|---------|-------------|
| `key` | string | Yes | — | Environment variable name (e.g., `AWS_ACCESS_KEY_ID`) |
| `description` | string | No | `""` | Human-readable description |
| `required` | boolean | No | `true` | Whether this variable must be set for the tool to function |

### install_commands

Shell commands targeting Debian bookworm (the sandbox base image). Multiline strings are supported — each non-empty, non-comment line is joined with ` && ` when generating the Docker `RUN` directive.

Example with multiple lines:
```
apt-get update
apt-get install -y azure-cli
rm -rf /var/lib/apt/lists/*
```

Becomes: `RUN apt-get update && apt-get install -y azure-cli && rm -rf /var/lib/apt/lists/*`

Lines starting with `#` are stripped as comments.

### check_command

A single command that exits with code 0 if the tool is installed. Used for:
- **Docker mode**: Skipping image rebuild if tool is already in the cached derived image
- **Local mode**: Verifying the tool is present on the host (local mode does NOT attempt installation)

### auth_script

Optional shell commands run inside the Docker container before `claude -p`. Can reference credential environment variables injected via workspace credential bindings.

When any tool in a job has an `auth_script`, the Docker entrypoint is overridden:
- A runner script is written to `/workspace/.claw-run.sh` containing the `claude -p` command
- The container runs: `bash -c "set -e; {auth_scripts}; exec /workspace/.claw-run.sh"`
- The prompt is never embedded in `bash -c` (prevents shell injection)

## manifest.json

Package metadata for import/export and marketplace listing.

```json
{
  "format": "claw-tool-v1",
  "id": "az-cli",
  "name": "Azure CLI",
  "version": "2.0.0",
  "author": "rob",
  "license": "MIT",
  "description": "Microsoft Azure command-line interface",
  "tags": ["cloud", "azure", "devops"]
}
```

### Fields

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| `format` | string | Yes | Must be `"claw-tool-v1"` |
| `id` | string | Yes | Unique identifier (e.g., `az-cli`, `aws-cli`) |
| `name` | string | Yes | Display name |
| `version` | string | No | Semantic version (e.g., `1.0.0`) |
| `author` | string | No | Package author |
| `license` | string | No | License identifier (e.g., `MIT`, `Apache-2.0`) |
| `description` | string | No | Short description |
| `tags` | string[] | No | Categorization tags |

### Import Behavior

When importing a ZIP:
1. `TOOL.json` is parsed for tool definition (install_commands, check_command, env_vars, auth_script)
2. `manifest.json` is parsed for metadata (id, name, version, author, license, description, tags)
3. Multipart form fields override both TOOL.json and manifest values
4. If no `TOOL.json`, tool definition fields must be provided via the API
5. If no `manifest.json`, metadata must be provided in the upload form

## Docker Derived Images

When a job requires tools, the worker:
1. Computes a content hash: `sha256(base_image + sorted tool IDs + install_commands)`
2. Checks for a cached derived image tagged `claw-tools:{hash_prefix}`
3. If not cached, generates a Dockerfile and builds:
   ```dockerfile
   FROM claw-sandbox:latest
   USER root
   RUN {tool1.install_commands joined with &&}
   RUN {tool2.install_commands joined with &&}
   ```
4. Caches the image for future jobs with the same tool set

## Credential Binding

Tools declare required env vars via `env_vars`. Credentials are stored separately (encrypted with AES-256-GCM) and bound to tools at the workspace level via `credential_bindings` (maps tool_id → credential_id). At job time, bound credentials are decrypted and injected as `-e KEY=VALUE` on the `docker run` command.

## API Endpoints

```
POST   /api/v1/tools/upload        — import tool from ZIP (multipart: file, id, name, description, tags)
GET    /api/v1/tools/{id}/download — export tool as ZIP
```

## Examples

### Minimal Tool ZIP

```
jq-tool.zip
├── TOOL.json
└── manifest.json
```

TOOL.json:
```json
{
  "install_commands": "apt-get update && apt-get install -y jq",
  "check_command": "jq --version"
}
```

manifest.json:
```json
{
  "format": "claw-tool-v1",
  "id": "jq-tool",
  "name": "jq JSON Processor"
}
```

### Full Tool ZIP (with auth)

```
az-cli.zip
├── TOOL.json
└── manifest.json
```

TOOL.json:
```json
{
  "install_commands": "curl -sL https://aka.ms/InstallAzureCLIDeb | bash",
  "check_command": "az --version",
  "env_vars": [
    {"key": "AZURE_CLIENT_ID", "description": "Service principal app ID", "required": true},
    {"key": "AZURE_CLIENT_SECRET", "description": "Service principal secret", "required": true},
    {"key": "AZURE_TENANT_ID", "description": "Azure tenant ID", "required": true}
  ],
  "auth_script": "az login --service-principal -u $AZURE_CLIENT_ID -p $AZURE_CLIENT_SECRET --tenant $AZURE_TENANT_ID"
}
```

manifest.json:
```json
{
  "format": "claw-tool-v1",
  "id": "az-cli",
  "name": "Azure CLI",
  "version": "1.0.0",
  "author": "rob",
  "license": "MIT",
  "description": "Microsoft Azure command-line interface",
  "tags": ["cloud", "azure"]
}
```
