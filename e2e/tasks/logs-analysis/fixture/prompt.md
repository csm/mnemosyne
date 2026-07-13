Analyze the nginx access logs in `/task/logs/` (combined log format, one
file per hour).

Write your answers to `/task/answers.json` with this schema:

```json
{
  "top_5_ips_by_bytes": ["<ip>", "<ip>", "<ip>", "<ip>", "<ip>"],
  "worst_5xx_window_start_utc": "<ISO-8601 UTC timestamp>",
  "worst_5xx_window_endpoint": "<path>",
  "scraper_user_agent": "<exact User-Agent string>",
  "credential_stuffing_distinct_ip_count": <integer>
}
```

- `top_5_ips_by_bytes`: the 5 client IPs responsible for the most total
  response bytes, ordered highest first.
- `worst_5xx_window_start_utc`: the UTC start time of the worst sustained
  5xx-error window, and the single endpoint responsible for it.
- `scraper_user_agent`: there is a bot crawling the site from a rotating
  set of IPs with one distinctive, fixed User-Agent string; give that exact
  string.
- `credential_stuffing_distinct_ip_count`: the number of distinct IPs
  making repeated failed login attempts against `/login` in a low-and-slow
  pattern (not a single burst).
