# Architecture Overview

Hotgraph starts as an event-native graph system. Writes enter as graph events, storage preserves the event log, indexes project queryable views, and API layers expose stable read/write boundaries.
