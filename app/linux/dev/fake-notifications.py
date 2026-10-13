#!/usr/bin/env python3
"""A stand-in for the desktop's notification server, for tests on a private session bus (dbus-run-session): it
records each Notify call as one JSON line in the file given as the first argument and, when CLICK_AFTER_MS is set,
clicks notification number CLICK_NTH (default 1) that long after it came (ActionInvoked "default")."""
import json
import os
import sys

from gi.repository import Gio, GLib

XML = """<node><interface name="org.freedesktop.Notifications">
<method name="Notify"><arg type="s" direction="in"/><arg type="u" direction="in"/><arg type="s" direction="in"/>
<arg type="s" direction="in"/><arg type="s" direction="in"/><arg type="as" direction="in"/>
<arg type="a{sv}" direction="in"/><arg type="i" direction="in"/><arg type="u" direction="out"/></method>
<method name="CloseNotification"><arg type="u" direction="in"/></method>
<method name="GetCapabilities"><arg type="as" direction="out"/></method>
<method name="GetServerInformation"><arg type="s" direction="out"/><arg type="s" direction="out"/>
<arg type="s" direction="out"/><arg type="s" direction="out"/></method>
<signal name="ActionInvoked"><arg type="u"/><arg type="s"/></signal>
<signal name="NotificationClosed"><arg type="u"/><arg type="u"/></signal>
</interface></node>"""

log = open(sys.argv[1], "a", buffering=1)
click_after = int(os.environ.get("CLICK_AFTER_MS", "0"))
click_nth = int(os.environ.get("CLICK_NTH", "1"))
state = {"next": 1, "clicked": False}


def call(conn, sender, path, iface, method, params, inv):
    if method == "Notify":
        app, _, icon, summary, body, actions, hints, timeout = params.unpack()
        nid = state["next"]
        state["next"] += 1
        log.write(json.dumps({"t": GLib.get_real_time() // 1000, "id": nid, "app": app, "icon": icon, "summary": summary, "body": body,
                              "actions": actions, "urgency": hints.get("urgency")}) + "\n")
        inv.return_value(GLib.Variant("(u)", (nid,)))
        if click_after and not state["clicked"] and nid == click_nth:
            state["clicked"] = True

            def click():
                conn.emit_signal(None, path, iface, "ActionInvoked", GLib.Variant("(us)", (nid, "default")))
                log.write(json.dumps({"clicked": nid}) + "\n")
                return False

            GLib.timeout_add(click_after, click)
    elif method == "GetCapabilities":
        inv.return_value(GLib.Variant("(as)", (["actions", "body"],)))
    elif method == "GetServerInformation":
        inv.return_value(GLib.Variant("(ssss)", ("fake", "trommi", "1", "1.2")))
    else:
        inv.return_value(None)


def on_bus(conn, name):
    node = Gio.DBusNodeInfo.new_for_xml(XML)
    conn.register_object("/org/freedesktop/Notifications", node.interfaces[0], call, None, None)


Gio.bus_own_name(Gio.BusType.SESSION, "org.freedesktop.Notifications", Gio.BusNameOwnerFlags.NONE, on_bus, None, None)
GLib.MainLoop().run()
