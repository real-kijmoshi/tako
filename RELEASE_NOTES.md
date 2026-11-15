# Tako 0.1.3

Requests now appear live in the dashboard while they run. Open dashboard tabs
also detect a new UI after a router restart and reload automatically, waiting
while a dialog is open or a field holds text.

The Connect screen now walks through choosing a tool, selecting a route, and
copying its settings. It keeps one client recipe in focus and guides new setups
to create a combo first.

Overview shows estimated tokens remaining from the latest provider limit report.
The estimate uses a single observation because targets and keys may share a quota;
it stays **Unknown** until a provider reports a limit.

Update with `tako update`. Updating restarts a running router and can interrupt
active requests; configuration and keys persist. Tabs opened on 0.1.2 or earlier
need one manual reload to receive the automatic reload support.
