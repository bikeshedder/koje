# koje's user layer (ADR 0040), run as root while the image is built. POSIX sh
# and tools every distribution has: the same on Fedora, Debian, Arch, …
set -eu
PATH=/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin
here=$(dirname "$0")
: "${USER_NAME:?}" "${USER_UID:?}" "${USER_HOME:?}"
USER_GID=${USER_GID:-$USER_UID}

# The host user with the same name, uid and home: contexts run with
# --userns=keep-id and mount the home at the host's path (ADR 0014). A user
# the image has with that uid (Ubuntu's "ubuntu") gives way.
other=$(cut -d: -f1,3 /etc/passwd | grep ":${USER_UID}\$" | cut -d: -f1 || true)
if [ -n "$other" ] && [ "$other" != "$USER_NAME" ]; then
    userdel "$other"
fi
# The user's group with the host's gid, so files keep their group: a group
# the image has with that gid is used; else one named after the user.
group=$(cut -d: -f1,3 /etc/group | grep ":${USER_GID}\$" | cut -d: -f1 || true)
if [ -z "$group" ]; then
    if cut -d: -f1 /etc/group | grep -qx "$USER_NAME"; then
        groupmod --gid "$USER_GID" "$USER_NAME"
    else
        groupadd --gid "$USER_GID" "$USER_NAME"
    fi
    group=$USER_NAME
fi
if cut -d: -f1 /etc/passwd | grep -qx "$USER_NAME"; then
    usermod --uid "$USER_UID" --gid "$group" --home "$USER_HOME" "$USER_NAME"
else
    useradd --uid "$USER_UID" --gid "$group" --home-dir "$USER_HOME" \
        --no-create-home "$USER_NAME"
fi

# Sub-ids for nested Podman: everything up to 65535 except the user's own
# uid (M2.5).
printf '%s:1:%s\n%s:%s:%s\n' \
    "$USER_NAME" "$((USER_UID - 1))" \
    "$USER_NAME" "$((USER_UID + 1))" "$((65535 - USER_UID))" \
    | tee /etc/subuid > /etc/subgid

# newuidmap/newgidmap need file capabilities for nested Podman; setuid does
# not work (M2.5). Images often lose them (Fedora's container image).
if command -v setcap >/dev/null 2>&1; then
    for pair in newuidmap:cap_setuid newgidmap:cap_setgid; do
        tool=$(command -v "${pair%%:*}" || true)
        if [ -n "$tool" ]; then
            chmod u-s "$tool"
            setcap "${pair#*:}=ep" "$tool"
        fi
    done
fi

# The inner Podman (ADR 0014): fuse-overlayfs storage, settings in
# containers.conf.
if command -v podman >/dev/null 2>&1; then
    mkdir -p /etc/containers
    cp "$here/containers.conf" /etc/containers/containers.conf
    printf '[storage]\ndriver = "overlay"\n[storage.options.overlay]\nmount_program = "%s"\n' \
        "$(command -v fuse-overlayfs || echo /usr/bin/fuse-overlayfs)" \
        > /etc/containers/storage.conf
fi

# The image's own portal must not be D-Bus activated on the context bus:
# koje's forwarder owns org.freedesktop.portal.Desktop there and talks to the
# host's portal (ADR 0019; M2.4 found the image's portal activated first).
rm -f /usr/share/dbus-1/services/org.freedesktop.portal.Desktop.service \
      /usr/share/dbus-1/services/org.freedesktop.impl.portal.*.service
