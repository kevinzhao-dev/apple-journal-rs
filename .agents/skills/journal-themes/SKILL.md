---
name: journal-themes
description: Organize Apple Journal entries by a topic such as food, travel, work, interests, or relationships using journal-rs. Use for thematic collections, personal preference summaries, and topic timelines, including requests framed around journal folders.
---

# Organize a topic

Read [the shared reading guide](../../../docs/agent-reading.md) before retrieving entries. Paths are relative to this file; these repository skills use the checkout's shared guide.

1. Identify the topic, period, and desired result: a collection of memories, a practical list, a timeline, or a reflection on preferences. Infer a suitable format when the request is clear.
2. For a named journal/folder, resolve its ID with `list_journals` (MCP) or `journals` (CLI), then apply the journal filter. Check the guide for name-decoding and local-versus-synced membership limitations. If an older tool lacks membership filtering, label content-based selection as approximate; exact folder requests need actual membership evidence.
3. Search literal topic terms, place/person names, and wording learned from relevant entries. For a bounded period, also inspect its entries for indirect mentions. Merge overlapping results by entry ID, then group multiple accounts of the same event. Read the supporting text before extracting facts.
4. Choose fields that serve the topic. Mark absent details as unrecorded and preserve ambiguity when similar names might refer to different places or people.
   - Food: venue, recorded visit/event date, dishes, companions when relevant, explicitly stated reactions, and revisit intention. Distinguish “said I want to return” from “returned”; a mention alone is not a recommendation.
   - Travel: destination, trip dates supported by text, sequence of activities, memorable moments, and preferences expressed about pace or arrangements. Distinguish planning from completed travel; use relevant location assets when available.
   - Work, interests, or relationships: episodes, changing priorities, turning points, and unanswered threads. Ground descriptions in the writer's account rather than inferring other people's motives.
5. Produce the collection with a source for each factual item. Add cross-entry observations only where the evidence supports them, together with exceptions. State the search scope and any unverified folder membership or missing records.

A topic may cross journal folders. Respect an explicit folder-only scope; when membership cannot be verified, report the limitation rather than silently broadening it. Default to a conversational result; follow the shared guide when the user requests a saved report.
