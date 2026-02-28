# Qdrant Vector Store Setup

## Prerequisites

- Docker and Docker Compose

## Start the Container

```bash
docker compose up -d
```

This starts a Qdrant instance with:
- **REST API** on `http://localhost:6333`
- **gRPC** on `localhost:6334`
- Persistent storage via a Docker volume (`qdrant_data`)

## Verify

```bash
curl http://localhost:6333/healthz
```

Should return `ok`.

## Dashboard

Qdrant ships a web UI at [http://localhost:6333/dashboard](http://localhost:6333/dashboard).

## Stop / Remove

```bash
docker compose down        # stop (data persists in volume)
docker compose down -v     # stop and delete stored data
```

## MCP Configuration

The project's `.mcp.json` includes `QDRANT_URL=http://localhost:6333` so the
MCP server can connect to Qdrant when a Qdrant-backed `VectorStore`
implementation is available.
