#!/bin/sh
# Stop leaves this marker before restarting the container, so it comes back
# with no agent running and the workspace holds still while it is exported.
if [ -e /run/foxfire-exporting ]; then
    exec tail -f /dev/null
fi
exec foxfire-relay -- aether acp
