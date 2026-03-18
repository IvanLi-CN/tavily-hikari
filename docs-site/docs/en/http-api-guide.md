# HTTP API Guide

## Public probes

| Method | Path           | Notes                  |
| ------ | -------------- | ---------------------- |
| `GET`  | `/health`      | Liveness probe         |
| `GET`  | `/api/summary` | Public summary metrics |

## Admin endpoints

| Method   | Path                   | Notes                          |
| -------- | ---------------------- | ------------------------------ |
| `GET`    | `/api/keys`            | List keys, status, counters    |
| `GET`    | `/api/logs?page=1`     | Paginated request logs         |
| `POST`   | `/api/keys`            | Add or restore a key           |
| `DELETE` | `/api/keys/:id`        | Soft-delete a key              |
| `GET`    | `/api/keys/:id/secret` | Reveal the upstream Tavily key |

## Hikari token endpoints

| Method | Path                 | Notes                                                |
| ------ | -------------------- | ---------------------------------------------------- |
| `POST` | `/api/tavily/search` | Tavily HTTP facade for clients such as Cherry Studio |
| `GET`  | `/api/user/token`    | Resolve the current user's bound access token        |
| `POST` | `/api/user/logout`   | End-user logout                                      |

## Cherry Studio setup

1. Create a Tavily Hikari access token from the user dashboard.
2. In Cherry Studio, choose the **Tavily (API key)** provider.
3. Set the API URL to `https://<your-host>/api/tavily`.
4. Use the Hikari token `th-<id>-<secret>` as the API key.

Do **not** paste the official Tavily API key into Cherry Studio when Hikari is in front of it.

## HTTP facade notes

- The Tavily HTTP facade keeps client-facing fields aligned with Tavily conventions where practical.
- Authentication is always Hikari-token-based, not upstream-key-based.
- Quota and routing decisions happen inside Hikari before the request reaches the upstream Tavily
  endpoint.

## When to use Storybook instead

If you are reviewing operator workflows or dashboard states rather than integrating an API client,
open [Storybook](/storybook.html) or the [Storybook Guide](/storybook-guide.html) instead of this
page.
