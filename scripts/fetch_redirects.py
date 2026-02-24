#!/usr/bin/env python3
"""
Fetch all redirects from PsychonautWiki and build a source->target mapping.

Phase 1: Get all redirect entries (fromid + target title) with pagination
Phase 2: Resolve fromid -> source title in batches of 50
Output: JSON file with redirect mappings
"""

import json
import time
import sys
import urllib.request
import urllib.parse

API_URL = "https://psychonautwiki.org/w/api.php"


def api_get(params):
    """Make a GET request to the MediaWiki API."""
    params["format"] = "json"
    url = API_URL + "?" + urllib.parse.urlencode(params)
    for attempt in range(3):
        try:
            req = urllib.request.Request(
                url, headers={"User-Agent": "Bifrost/4.0 redirect-fetcher"}
            )
            with urllib.request.urlopen(req, timeout=30) as resp:
                return json.loads(resp.read().decode())
        except Exception as e:
            print(f"  Attempt {attempt + 1} failed: {e}", file=sys.stderr)
            if attempt < 2:
                time.sleep(2**attempt)
    raise Exception(f"Failed after 3 attempts: {url}")


def phase1_fetch_all_redirects():
    """Phase 1: Get all redirect entries with fromid and target title."""
    print("Phase 1: Fetching all redirect entries...", file=sys.stderr)

    all_entries = []  # List of (fromid, target_title)
    params = {
        "action": "query",
        "list": "allredirects",
        "arprop": "ids|title",
        "arnamespace": "0",
        "arlimit": "max",
    }

    batch = 0
    while True:
        batch += 1
        print(
            f"  Batch {batch}... ({len(all_entries)} entries so far)", file=sys.stderr
        )

        data = api_get(params)

        redirects = data.get("query", {}).get("allredirects", [])
        for entry in redirects:
            fromid = entry.get("fromid")
            title = entry.get("title")
            if fromid and title:
                all_entries.append((fromid, title))

        # Check for continuation
        cont = data.get("continue")
        if cont and "arcontinue" in cont:
            params["arcontinue"] = cont["arcontinue"]
            params["continue"] = cont.get("continue", "-||")
            time.sleep(0.5)  # Rate limiting
        else:
            break

    print(f"  Phase 1 complete: {len(all_entries)} entries", file=sys.stderr)
    return all_entries


def phase2_resolve_page_ids(entries):
    """Phase 2: Resolve fromid -> source page title in batches."""
    print("Phase 2: Resolving page IDs to titles...", file=sys.stderr)

    # Collect unique fromids
    unique_ids = list(set(fromid for fromid, _ in entries))
    print(f"  {len(unique_ids)} unique page IDs to resolve", file=sys.stderr)

    # Build fromid -> source_title mapping
    id_to_title = {}

    # Process in batches of 50
    batch_size = 50
    for i in range(0, len(unique_ids), batch_size):
        batch_ids = unique_ids[i : i + batch_size]
        batch_num = i // batch_size + 1
        total_batches = (len(unique_ids) + batch_size - 1) // batch_size

        if batch_num % 10 == 1:
            print(
                f"  Batch {batch_num}/{total_batches}... ({len(id_to_title)} resolved)",
                file=sys.stderr,
            )

        params = {
            "action": "query",
            "pageids": "|".join(str(pid) for pid in batch_ids),
            "prop": "info",
        }

        data = api_get(params)

        pages = data.get("query", {}).get("pages", {})
        for pid_str, page_info in pages.items():
            pid = int(pid_str)
            if pid > 0 and "title" in page_info:
                id_to_title[pid] = page_info["title"]

        time.sleep(0.3)  # Rate limiting

    print(f"  Phase 2 complete: {len(id_to_title)} titles resolved", file=sys.stderr)
    return id_to_title


def build_redirect_map(entries, id_to_title):
    """Build the final source -> target mapping."""
    print("Building redirect map...", file=sys.stderr)

    # source_title -> target_title
    redirect_map = {}

    for fromid, target_title in entries:
        source_title = id_to_title.get(fromid)
        if source_title and source_title != target_title:
            # A redirect page: source_title redirects to target_title
            if target_title not in redirect_map:
                redirect_map[target_title] = []
            if source_title not in redirect_map[target_title]:
                redirect_map[target_title].append(source_title)

    # Sort the lists for determinism
    for target in redirect_map:
        redirect_map[target].sort()

    # Sort by target name
    redirect_map = dict(sorted(redirect_map.items()))

    print(f"  {len(redirect_map)} targets with redirects", file=sys.stderr)
    total_redirects = sum(len(v) for v in redirect_map.values())
    print(f"  {total_redirects} total redirect entries", file=sys.stderr)

    return redirect_map


def main():
    print("PsychonautWiki Redirect Fetcher", file=sys.stderr)
    print("=" * 40, file=sys.stderr)

    # Phase 1
    entries = phase1_fetch_all_redirects()

    # Phase 2
    id_to_title = phase2_resolve_page_ids(entries)

    # Build map: target -> [source1, source2, ...]
    redirect_map = build_redirect_map(entries, id_to_title)

    # Output JSON
    output = {
        "_meta": {
            "description": "PsychonautWiki redirect mappings: target -> [source aliases]",
            "generated_at": time.strftime("%Y-%m-%dT%H:%M:%SZ", time.gmtime()),
            "total_targets": len(redirect_map),
            "total_redirects": sum(len(v) for v in redirect_map.values()),
        },
        "redirects": redirect_map,
    }

    print(json.dumps(output, indent=2, ensure_ascii=False))

    print("\nDone!", file=sys.stderr)


if __name__ == "__main__":
    main()
