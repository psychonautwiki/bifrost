# Bifrost

```
          /\,%_\
          \%/,\          Old age should burn
       _.-"%%|//%    and rave at close of day
      .'  .-"  /%%%
  _.-'_.-" 0)   \%%%     Rage, rage against
 /.\.'          \%%%  the dying of the light
 \ /      _,      %%%
  `"--"~`\   _,*'\%'   _,--""""-,%%,
         )*^     `""~~`           \%%%,
         _/                          \%%%
     _.-`/                           |%%,___
 _.-"   /      ,              ,     ,|%%   .`\
/\     /      /                `\     \%'   \ /
\ \ _,/      /`~.-._          _,`\     \`""~~`
 `"` /-.`_, /'      `~----"~     `\     \
     \___,'                        \.-"`/
                                    `--'
```

GraphQL API server for [PsychonautWiki](https://psychonautwiki.org). Fetches substance data from the wiki's MediaWiki API and exposes it through a typed GraphQL interface with stale-while-revalidate caching.

## Quick Start

```bash
# Run with cargo
cargo run

# Or with Docker
docker build -t bifrost .
docker run -p 3000:3000 bifrost
```

Open http://localhost:3000 for the GraphiQL playground.

## Example Queries

```graphql
# Search substances
{
  substances(query: "psilocybin", limit: 5) {
    name
    summary
    class { psychoactive chemical }
    rpioa { oral { dose { threshold light common strong heavy } } }
    dangerousInteractions { name }
  }
}

# Get effects for a substance
{
  effectsBySubstance(substance: "LSD") {
    name
    url
  }
}

# Find substances by effect
{
  substancesByEffect(effect: ["Euphoria", "Time distortion"]) {
    name
  }
}
```

## Configuration

| Variable | Default | Description |
|----------|---------|-------------|
| `PORT` | `3000` | Server port |
| `CACHE_TTL_MS` | `86400000` (24h) | Cache TTL in milliseconds |
| `PLEBISCITE` | (unset) | Enable Erowid experience reports (requires MongoDB) |
| `MONGO_URL` | - | MongoDB connection string (required if `PLEBISCITE` set) |
| `MONGO_DB` | `bifrost` | MongoDB database name |
| `MONGO_COLLECTION` | `plebiscite` | MongoDB collection name |

### CLI Options

```
bifrost [OPTIONS]

Options:
  -l, --log-level <LEVEL>  Log level [default: info]
  -p, --port <PORT>        Override PORT env var
      --json-logs          JSON log format
      --debug-requests     Log upstream API requests
```

## Architecture

- **axum** + **async-graphql** for the HTTP/GraphQL layer
- **Stale-while-revalidate cache** with request coalescing - returns stale data immediately while refreshing in the background
- Upstream data from PsychonautWiki's MediaWiki API (parsed from wikitext)
- Optional MongoDB integration for Erowid experience reports via the `erowid` query

## License

MIT - Copyright (c) 2016-2025 Kenan Sulayman / PsychonautWiki
