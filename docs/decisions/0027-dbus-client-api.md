# 0027. Client API: kojed on the session bus

- Status: Accepted
- Date: 2026-10-19
- Source: plan task M4.1, discussion

## Context

`koje`, the bar module (M8.2) and the panel (M8.4) talk to kojed: commands
(focus a context, switch a layout) and events (a window appeared, a context went
to sleep). The bar and panel want to subscribe to events.

## Decision

- kojed owns **`io.koje.Daemon`** on the user's **session bus**, object
  `/io/koje/Daemon`, interface **`io.koje.Daemon1`** (version in the name; a
  breaking change gets `…2`).
- Methods for commands (first: `Focus(s context)`, `ListContexts()`,
  `Status()`); signals for events (`WindowAdded`, `WindowRemoved`,
  `ContextChanged`). The live set is whatever `busctl --user introspect`
  shows; M4.7 started with `ListContexts`, `ActiveContext`, `Reload` and the
  two window signals.
- Contexts never reach it: they have their own session bus, and the filtered
  host bus (ADR 0019) allows only portal and notification names.
- Clients that find no owner of `io.koje.Daemon` say that kojed is not running
  and how to start it.

## Consequences

+ Events for free (D-Bus signals); `busctl --user introspect io.koje.Daemon
  /io/koje/Daemon` documents the live API; any language can be a client.
- Needs a session bus (normal on desktop distributions).
