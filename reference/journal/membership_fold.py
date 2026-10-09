"""Journal spec §9.12's fold, as a reference implementation with history vectors (DEC-437 item 9,
DEC-648): the membership state, the effective roles, and identity spec §5.3's `workspace_users` that
the control stream's membership records fold to.

The `membership_fold` section holds histories: control-stream records in `seq` order, each a
membership record valid under §9.12's rules (checked with `control.violations`), or another record
by a client, an agent, or a service account, which the fold ignores. Each history states, at named
instants, the expected per-member and per-invitation state, each member's effective roles, and the
count. The expectations are written by hand from identity spec §5 and §8.3, not computed by this
fold, so the fold and the vectors check each other. Out-of-state histories read as unreadable, and
their count as 1. Seeded fold bugs are shown caught. The identity crate's fold (E9-7, M1) is tested
against the same vectors.
"""

from __future__ import annotations

import copy
from dataclasses import dataclass, field
from datetime import datetime, timedelta

from control import STREAM, instant_nanos, violations

SPEC = "docs/specs/journal.md §9.12 the fold (DEC-437 item 9, DEC-648)"
COOLING = ("approver", "operator")
LIVE = ("cooling_off", "active")
T0 = datetime(2026, 10, 5, 14, 0, 0)
FOUNDER = "01J8Z5F0A000000000000000F1"
B = "01J8Z5F0B000000000000000F2"
C = "01J8Z5F0C000000000000000F3"
INV = {n: f"01J8Z5F1{n}00000000000000000"[:26] for n in ("A", "B", "C")}
SERVICES = {"kind": "system", "id": "control_services", "version": "0.1.0", "build": "sha256:" + "d" * 64}
CLIENT = {"kind": "client", "id": "client_b_01", "version": "1", "build": None}
AGENT = {"kind": "agent", "id": "agent_a", "version": "0.1.0", "build": "sha256:" + "c" * 64}
SERVICE_ACCOUNT = {"kind": "system", "id": "svc_export_01", "version": "1", "build": "sha256:" + "e" * 64}


def ts(at: datetime, nanos: int = 0) -> str:
    """A §4.7 timestamp `nanos` nanoseconds after `at` (negative before it)."""
    whole, rest = divmod(nanos, 10**9)
    moment = at + timedelta(seconds=whole)
    return moment.strftime("%Y-%m-%dT%H:%M:%S.") + f"{rest:09d}Z"


def hours(n: int) -> datetime:
    return T0 + timedelta(hours=n)


def user(principal: str) -> dict:
    return {"kind": "user", "id": principal, "version": "1", "build": None}


def step_up(at: datetime) -> dict:
    return {"assertion_id": f"assert_{at:%H%M}", "authenticated_at": ts(at, -10 * 10**9), "method": "passkey"}


def record(event_type: str, at: datetime, actor: dict, payload: dict) -> dict:
    session = f"session_{actor['id'][-4:]}" if actor["kind"] == "user" else None
    return {"event_type": event_type, "event_time": ts(at), "actor": dict(actor), "payload": payload | {"session_ref": session}}


def founding(at: datetime, member: str = FOUNDER, independent: bool = True) -> dict:
    return record(
        "MemberActivated",
        at,
        SERVICES,
        {
            "member": member,
            "invitation": None,
            "reason": "founding",
            "roles": ["approver", "operator", "workspace_admin"],
            "method": "passkey",
            "activated_at": ts(at),
            "independent_approval_required": independent,
            "cool_off_ends_at": ts(at),
        },
    )


def invited(at: datetime, invitation: str, roles: list[str], by: str = FOUNDER) -> dict:
    return record(
        "MemberInvited",
        at,
        user(by),
        {
            "invitation": invitation,
            "roles": roles,
            "invited_by": by,
            "step_up": step_up(at),
            "invited_at": ts(at),
            "expires_at": ts(at + timedelta(days=7)),
        },
    )


def revoked(at: datetime, invitation: str, by: str = FOUNDER) -> dict:
    return record("MemberInvitationRevoked", at, user(by), {"invitation": invitation, "revoked_by": by})


def cool_end(at: datetime, independent: bool, roles) -> str:
    return ts(at + timedelta(days=1)) if independent and set(roles) & set(COOLING) else ts(at)


def accepted(at: datetime, member: str, invitation: str, roles: list[str], independent: bool = True) -> dict:
    return record(
        "MemberActivated",
        at,
        user(member),
        {
            "member": member,
            "invitation": invitation,
            "reason": "invitation_accepted",
            "roles": roles,
            "method": "oidc",
            "activated_at": ts(at),
            "independent_approval_required": independent,
            "cool_off_ends_at": cool_end(at, independent, roles),
        },
    )


def role_changed(at: datetime, member: str, added: list[str], removed: list[str], independent: bool = True) -> dict:
    return record(
        "MemberRoleChanged",
        at,
        user(FOUNDER),
        {
            "member": member,
            "changed_by": FOUNDER,
            "added": [{"role": r, "cool_off_ends_at": cool_end(at, independent, [r])} for r in added],
            "removed": removed,
            "changed_at": ts(at),
            "independent_approval_required": independent,
            "step_up": step_up(at) if added else None,
        },
    )


def deactivated(at: datetime, member: str, reason: str = "admin") -> dict:
    by = member if reason == "left" else FOUNDER
    return record("MemberDeactivated", at, user(by), {"member": member, "by": by, "reason": reason})


def reactivated(at: datetime, member: str, roles: list[str], independent: bool = True) -> dict:
    return record(
        "MemberReactivated",
        at,
        user(FOUNDER),
        {
            "member": member,
            "by": FOUNDER,
            "step_up": step_up(at),
            "roles": roles,
            "reactivated_at": ts(at),
            "independent_approval_required": independent,
            "cool_off_ends_at": cool_end(at, independent, roles),
        },
    )


def removed(at: datetime, member: str) -> dict:
    return record("MemberRemoved", at, user(FOUNDER), {"member": member, "by": FOUNDER, "reason": "admin"})


def other(at: datetime, actor: dict, event_type: str = "OwnerCommandIssued") -> dict:
    """A record by a principal that holds no membership; the fold ignores it."""
    return {"event_type": event_type, "event_time": ts(at), "actor": dict(actor), "payload": {}}


def probe(at: str, users: int, members: dict, roles: dict | None = None, invitations: dict | None = None) -> dict:
    """A probe's expectations as lists sorted by ID: journal keys are lowercase (§4.1), and a ULID is
    not, so no map is keyed by one."""
    return {
        "at": at,
        "workspace_users": users,
        "members": [{"member": m, "state": v} for m, v in sorted(members.items())],
        "roles": [{"member": m, "roles": v} for m, v in sorted((roles or {}).items())],
        "invitations": [{"invitation": i, "state": v} for i, v in sorted((invitations or {}).items())],
    }


# --------------------------------------------------------------------------- the histories


def histories() -> list[dict]:
    """Every expectation is written from identity spec §5 and §8.3 by hand."""
    h1, h2, h3 = hours(1), hours(2), hours(3)
    day_after_h2 = ts(h2 + timedelta(days=1))
    return [
        {
            "name": "invited_cooling_then_active",
            "clause": "§5.1, §5.3, §8.3: invited and cooling-off count zero; a client counts zero",
            "records": [
                founding(T0),
                invited(h1, INV["A"], ["approver", "viewer"]),
                other(h1, CLIENT),
                other(h1, AGENT, "AgentDeployed"),
                other(h1, SERVICE_ACCOUNT, "ExportCreated"),
                accepted(h2, B, INV["A"], ["approver", "viewer"]),
            ],
            "unreadable": False,
            "probes": [
                probe(ts(h1), 1, {FOUNDER: "active"}, {FOUNDER: ["approver", "operator", "workspace_admin"]}, {INV["A"]: "invited"}),
                probe(ts(h2), 1, {FOUNDER: "active", B: "cooling_off"}, {B: ["viewer"]}, {INV["A"]: "accepted"}),
                probe(ts(h2 + timedelta(days=1), -1), 1, {B: "cooling_off"}, {B: ["viewer"]}),
                probe(day_after_h2, 2, {B: "active"}, {B: ["approver", "viewer"]}),
            ],
        },
        {
            "name": "per_role_cool_off",
            "clause": "§8.3: each added role is effective from its own end; a removal is at once",
            "records": [
                founding(T0),
                invited(h1, INV["A"], ["viewer"]),
                accepted(h2, B, INV["A"], ["viewer"]),
                role_changed(h3, B, ["auditor", "operator"], ["viewer"]),
            ],
            "unreadable": False,
            "probes": [
                probe(ts(h2), 2, {B: "active"}, {B: ["viewer"]}),
                probe(ts(h3), 2, {B: "active"}, {B: ["auditor"]}),
                probe(ts(h3 + timedelta(days=1)), 2, {B: "active"}, {B: ["auditor", "operator"]}),
            ],
        },
        {
            "name": "no_independence_no_cool_off",
            "clause": "§8.3: without independence required, an approver is active at once",
            "records": [
                founding(T0, independent=False),
                invited(h1, INV["A"], ["approver"]),
                accepted(h2, B, INV["A"], ["approver"], independent=False),
            ],
            "unreadable": False,
            "probes": [probe(ts(h2), 2, {B: "active"}, {B: ["approver"]})],
        },
        {
            "name": "deactivate_reactivate_leave_remove",
            "clause": "§5.1: deactivated and removed count zero; a reactivation restores the kept roles",
            "records": [
                founding(T0),
                invited(h1, INV["A"], ["approver", "viewer"]),
                accepted(h2, B, INV["A"], ["approver", "viewer"], independent=False),
                deactivated(hours(4), B),
                reactivated(hours(5), B, ["approver", "viewer"]),
                deactivated(hours(30), B, "left"),
                removed(hours(31), B),
            ],
            "unreadable": False,
            "probes": [
                probe(ts(hours(4)), 1, {B: "deactivated"}, {B: []}),
                probe(ts(hours(5)), 1, {B: "cooling_off"}, {B: ["viewer"]}),
                probe(ts(hours(29)), 2, {B: "active"}, {B: ["approver", "viewer"]}),
                probe(ts(hours(30)), 1, {B: "deactivated"}, {B: []}),
                probe(ts(hours(31)), 1, {B: "removed"}, {B: []}),
            ],
        },
        {
            "name": "invitations_expire_and_are_revoked",
            "clause": "§5.1: an invitation expires at `expires_at`; a revoked one is terminal",
            "records": [
                founding(T0),
                invited(h1, INV["A"], ["viewer"]),
                invited(h1, INV["B"], ["viewer"]),
                revoked(h2, INV["B"]),
            ],
            "unreadable": False,
            "probes": [
                probe(ts(h1 + timedelta(days=7), -1), 1, {FOUNDER: "active"}, {}, {INV["A"]: "invited", INV["B"]: "revoked"}),
                probe(ts(h1 + timedelta(days=7)), 1, {}, {}, {INV["A"]: "expired", INV["B"]: "revoked"}),
            ],
        },
        {
            "name": "reinvited_after_removal",
            "clause": "§9.12: a removed member returns only by a new invitation, with none of the old roles",
            "records": [
                founding(T0),
                invited(h1, INV["A"], ["viewer"]),
                accepted(h2, B, INV["A"], ["viewer"]),
                deactivated(hours(4), B),
                removed(hours(5), B),
                invited(hours(6), INV["B"], ["auditor"]),
                accepted(hours(7), B, INV["B"], ["auditor"]),
            ],
            "unreadable": False,
            "probes": [
                probe(ts(hours(6)), 1, {B: "removed"}, {B: []}, {INV["A"]: "accepted", INV["B"]: "invited"}),
                probe(ts(hours(7)), 2, {B: "active"}, {B: ["auditor"]}, {INV["B"]: "accepted"}),
            ],
        },
        {
            "name": "changed_and_deactivated_while_cooling_off",
            "clause": "§9.12: a member in `cooling_off` may have roles changed and be deactivated",
            "records": [
                founding(T0),
                invited(h1, INV["A"], ["approver", "viewer"]),
                accepted(h2, B, INV["A"], ["approver", "viewer"]),
                role_changed(h3, B, ["auditor"], []),
                deactivated(hours(4), B),
            ],
            "unreadable": False,
            "probes": [
                probe(ts(h3), 1, {B: "cooling_off"}, {B: ["auditor", "viewer"]}),
                probe(ts(hours(4)), 1, {B: "deactivated"}, {B: []}),
                probe(day_after_h2, 1, {B: "deactivated"}, {B: []}),
            ],
        },
        {
            "name": "roles_removed_while_deactivated",
            "clause": "§9.12, DEC-654 item 7: a removal-only change offboards a deactivated member's kept roles",
            "records": [
                founding(T0),
                invited(h1, INV["A"], ["approver", "viewer"]),
                accepted(h2, B, INV["A"], ["approver", "viewer"], independent=False),
                deactivated(hours(4), B),
                role_changed(hours(5), B, [], ["approver"]),
                reactivated(hours(6), B, ["viewer"]),
            ],
            "unreadable": False,
            "probes": [
                probe(ts(hours(5)), 1, {B: "deactivated"}, {B: []}),
                probe(ts(hours(6)), 2, {B: "active"}, {B: ["viewer"]}),
            ],
        },
        *[
            {
                "name": name,
                "clause": clause,
                "records": records,
                "unreadable": True,
                "probes": [probe(ts(hours(400)), 1, {})],
            }
            for name, clause, records in unreadable_histories()
        ],
    ]


def unreadable_histories() -> list[tuple[str, str, list[dict]]]:
    """Histories with a record that does not fit the state it finds: the count reads 1 whatever the
    other members' states, even with two active users."""
    h1, h2 = hours(1), hours(2)
    two_users = [founding(T0), invited(h1, INV["A"], ["viewer"]), accepted(h2, B, INV["A"], ["viewer"])]
    return [
        (
            "activated_at_the_expiry",
            "§9.12: `activated_at` < `expires_at`",
            [founding(T0), invited(h1, INV["A"], ["viewer"]), accepted(h1 + timedelta(days=7), B, INV["A"], ["viewer"])],
        ),
        (
            "a_revoked_invitation_accepted",
            "§9.12: revoked is terminal",
            [founding(T0), invited(h1, INV["A"], ["viewer"]), revoked(h2, INV["A"]), accepted(hours(3), B, INV["A"], ["viewer"])],
        ),
        (
            "an_invitation_accepted_twice",
            "§9.12: an invitation activates at most once",
            [*two_users, accepted(hours(3), C, INV["A"], ["viewer"])],
        ),
        (
            "a_second_activation",
            "§9.12: a member whose membership is not removed is not activated again",
            [*two_users, invited(hours(3), INV["B"], ["auditor"]), accepted(hours(4), B, INV["B"], ["auditor"])],
        ),
        (
            "roles_other_than_the_invitation",
            "§9.12",
            [founding(T0), invited(h1, INV["A"], ["viewer"]), accepted(h2, B, INV["A"], ["auditor"])],
        ),
        ("removed_while_active", "§9.12: only a deactivated member is removed", [*two_users, removed(hours(3), B)]),
        (
            "removed_then_reactivated",
            "§9.12: removed is terminal",
            [*two_users, deactivated(hours(3), B), removed(hours(4), B), reactivated(hours(5), B, ["viewer"])],
        ),
        (
            "reactivated_with_other_roles",
            "§9.12: a reactivation restores exactly the kept roles",
            [*two_users, deactivated(hours(3), B), reactivated(hours(4), B, ["auditor"])],
        ),
        (
            "a_role_removed_that_is_not_held",
            "§9.12",
            [*two_users, role_changed(hours(3), B, [], ["auditor"])],
        ),
        (
            "a_role_added_that_is_held",
            "§9.12",
            [*two_users, role_changed(hours(3), B, ["viewer"], [])],
        ),
        (
            "a_role_change_while_deactivated",
            "§9.12",
            [*two_users, deactivated(hours(3), B), role_changed(hours(4), B, ["auditor"], [])],
        ),
        (
            "a_role_granted_while_deactivated",
            "§9.12, DEC-654 item 7: a grant never reaches a deactivated member, even with a removal",
            [*two_users, deactivated(hours(3), B), role_changed(hours(4), B, ["auditor"], ["viewer"])],
        ),
        (
            "reactivated_with_a_stripped_role",
            "§9.12: a kept role removed while deactivated is not restored",
            [*two_users, deactivated(hours(3), B), role_changed(hours(4), B, [], ["viewer"]), reactivated(hours(5), B, ["viewer"])],
        ),
        (
            "a_role_change_after_removal",
            "§9.12: removed is terminal",
            [*two_users, deactivated(hours(3), B), removed(hours(4), B), role_changed(hours(5), B, ["auditor"], [])],
        ),
        (
            "deactivated_twice",
            "§9.12",
            [*two_users, deactivated(hours(3), B), deactivated(hours(4), B)],
        ),
        (
            "an_unknown_invitation_accepted",
            "§9.12",
            [founding(T0), accepted(h2, B, INV["C"], ["viewer"])],
        ),
        (
            "an_invitation_issued_twice",
            "§9.12: an invitation's ULID is issued once",
            [founding(T0), invited(h1, INV["A"], ["viewer"]), invited(h2, INV["A"], ["auditor"])],
        ),
        (
            "a_revocation_of_an_accepted_invitation",
            "§9.12: only an `invited` invitation is revoked",
            [*two_users, revoked(hours(3), INV["A"])],
        ),
        (
            "a_revocation_of_an_expired_invitation",
            "§9.12: an invitation is `expired` from its `expires_at`",
            [founding(T0), invited(h1, INV["A"], ["viewer"]), revoked(h1 + timedelta(days=7), INV["A"])],
        ),
        (
            "a_revocation_of_an_unknown_invitation",
            "§9.12",
            [founding(T0), revoked(h1, INV["C"])],
        ),
        (
            "a_role_change_of_an_unknown_member",
            "§9.12",
            [founding(T0), role_changed(h1, C, ["auditor"], [])],
        ),
        (
            "a_deactivation_of_an_unknown_member",
            "§9.12",
            [founding(T0), deactivated(h1, C)],
        ),
    ]


# --------------------------------------------------------------------------- the fold


@dataclass
class Member:
    status: str
    until: int
    roles: dict[str, int] = field(default_factory=dict)
    kept: list[str] = field(default_factory=list)


@dataclass
class Fold:
    """The fold's state after the records so far. `unreadable` latches."""

    invitations: dict[str, dict] = field(default_factory=dict)
    members: dict[str, Member] = field(default_factory=dict)
    unreadable: bool = False
    skip: frozenset[str] = frozenset()
    clients: set[str] = field(default_factory=set)

    def refuse(self) -> None:
        self.unreadable = True

    def unknown_member(self) -> None:
        """A record about a member the fold has never activated."""
        if "fold.unknown_member_tolerated" not in self.skip:
            self.refuse()

    def apply(self, rec: dict) -> None:
        kind = rec["event_type"]
        p = rec["payload"]
        at = instant_nanos(rec["event_time"])
        if not kind.startswith("Member"):
            if rec["actor"]["kind"] == "client":
                self.clients.add(rec["actor"]["id"])
            return
        if kind == "MemberInvited":
            if p["invitation"] in self.invitations and "fold.duplicate_invitation_tolerated" not in self.skip:
                return self.refuse()
            self.invitations[p["invitation"]] = {
                "state": "invited",
                "roles": list(p["roles"]),
                "expires_at": instant_nanos(p["expires_at"]),
            }
        elif kind == "MemberInvitationRevoked":
            inv = self.invitations.get(p["invitation"])
            if inv is None:
                return None if "fold.revoke_unknown_tolerated" in self.skip else self.refuse()
            if not self.open(inv, at) and "fold.revoke_any_state" not in self.skip:
                return self.refuse()
            if "fold.revocation_ignored" not in self.skip:
                inv["state"] = "revoked"
        elif kind == "MemberActivated":
            self.activate(p, at)
        elif kind == "MemberRoleChanged":
            m = self.members.get(p["member"])
            if m is None:
                return self.unknown_member()
            if m.status == "deactivated":
                return self.change_kept(m, p)
            if m.status not in LIVE and "fold.role_change_any_state" not in self.skip:
                return self.refuse()
            if any(r not in m.roles for r in p["removed"]) and "fold.unheld_role_removed" not in self.skip:
                return self.refuse()
            if any(a["role"] in m.roles for a in p["added"]) and "fold.held_role_added" not in self.skip:
                return self.refuse()
            for r in p["removed"]:
                m.roles.pop(r, None)
            for a in p["added"]:
                end = at if "fold.per_role_cool_off_ignored" in self.skip else instant_nanos(a["cool_off_ends_at"])
                m.roles[a["role"]] = end
        elif kind == "MemberDeactivated":
            m = self.members.get(p["member"])
            if m is None:
                return self.unknown_member()
            if m.status not in LIVE and "fold.deactivate_any_state" not in self.skip:
                return self.refuse()
            m.kept, m.roles, m.status = sorted(m.roles), {}, "deactivated"
        elif kind == "MemberReactivated":
            m = self.members.get(p["member"])
            reactivatable = ("deactivated", "removed") if "fold.removed_reactivates" in self.skip else ("deactivated",)
            kept = list(p["roles"]) == m.kept if m else False
            if m is None or m.status not in reactivatable or not (kept or "fold.reactivation_roles_unchecked" in self.skip):
                return self.refuse()
            self.start(m, p["roles"], at, instant_nanos(p["cool_off_ends_at"]))
        elif kind == "MemberRemoved":
            m = self.members.get(p["member"])
            if m is None or (m.status != "deactivated" and "fold.remove_any_state" not in self.skip):
                return self.refuse()
            m.status = "removed"
        return None

    def change_kept(self, m: Member, p: dict) -> None:
        """DEC-654 item 7: removing kept roles from a `deactivated` member only reduces access, so it
        is accepted for offboarding; a grant to one is refused, as `change_roles` refuses it."""
        if p["added"] and "fold.deactivated_grant_accepted" not in self.skip:
            return self.refuse()
        if not p["added"] and "fold.deactivated_removal_refused" in self.skip:
            return self.refuse()
        if any(r not in m.kept for r in p["removed"]):
            return self.refuse()
        m.kept = sorted({*m.kept, *(a["role"] for a in p["added"])} - set(p["removed"]))
        return None

    def open(self, inv: dict, at: int) -> bool:
        """An invitation still `invited` at `at`: §9.12's `activated_at` < `expires_at`."""
        if inv["state"] != "invited":
            return False
        if "fold.expiry_inclusive" in self.skip:
            return at <= inv["expires_at"]
        return at < inv["expires_at"]

    def activate(self, p: dict, at: int) -> None:
        previous = self.members.get(p["member"])
        if previous is not None and previous.status != "removed" and "fold.second_activation_tolerated" not in self.skip:
            return self.refuse()
        if p["reason"] == "invitation_accepted":
            inv = self.invitations.get(p["invitation"])
            if inv is None:
                return self.refuse()
            reused = inv["state"] == "accepted" and "fold.invitation_reused" in self.skip
            if not (self.open(inv, at) or reused):
                return self.refuse()
            if inv["roles"] != list(p["roles"]) and "fold.invitation_roles_unchecked" not in self.skip:
                return self.refuse()
            inv["state"] = "accepted"
        m = Member(status="cooling_off", until=0)
        if previous is not None and "fold.old_roles_kept" in self.skip:
            m.roles = {r: at for r in previous.kept} or {}
        self.members[p["member"]] = m
        self.start(m, p["roles"], at, instant_nanos(p["cool_off_ends_at"]))
        return None

    def start(self, m: Member, roles: list[str], at: int, end: int) -> None:
        """A membership enters `cooling_off` until `end`; a cooling role is effective from `end`."""
        if "fold.cool_off_ignored" in self.skip:
            end = at
        m.status, m.until, m.kept = "cooling_off", end, []
        for r in roles:
            m.roles[r] = end if r in COOLING else at

    def member_state(self, member: str, at: int) -> str:
        m = self.members[member]
        if m.status in LIVE:
            return "active" if at >= m.until else "cooling_off"
        return m.status

    def invitation_state(self, invitation: str, at: int) -> str:
        inv = self.invitations[invitation]
        if inv["state"] == "invited" and at >= inv["expires_at"]:
            return "expired"
        return inv["state"]

    def effective_roles(self, member: str, at: int) -> list[str]:
        m = self.members[member]
        if m.status not in LIVE:
            return []
        return sorted(r for r, since in m.roles.items() if since <= at)

    def workspace_users(self, at: int) -> int:
        """Identity spec §5.3: distinct users active and past their cool-off; unreadable is 1."""
        if self.unreadable and "fold.unreadable_not_one" not in self.skip:
            return 1
        counted = {"active", "cooling_off"} if "fold.cooling_counts" in self.skip else {"active"}
        if "fold.deactivated_counts" in self.skip:
            counted |= {"deactivated"}
        users = sum(1 for member in self.members if self.member_state(member, at) in counted)
        if "fold.invited_counts" in self.skip:
            users += sum(1 for inv in self.invitations if self.invitation_state(inv, at) == "invited")
        if "fold.client_counted" in self.skip:
            users += len(self.clients)
        return users


def fold(records: list[dict], skip: frozenset[str] = frozenset()) -> Fold:
    state = Fold(skip=skip)
    for rec in records:
        state.apply(rec)
    return state


def observed(state: Fold, want: dict) -> dict:
    """What the fold reads at a probe, for exactly the members, roles and invitations it names."""
    at = instant_nanos(want["at"])
    return {
        "at": want["at"],
        "workspace_users": state.workspace_users(at),
        "members": [
            {"member": e["member"], "state": state.member_state(e["member"], at)}
            for e in want["members"]
            if e["member"] in state.members
        ],
        "roles": [
            {"member": e["member"], "roles": state.effective_roles(e["member"], at)}
            for e in want["roles"]
            if e["member"] in state.members
        ],
        "invitations": [
            {"invitation": e["invitation"], "state": state.invitation_state(e["invitation"], at)}
            for e in want["invitations"]
            if e["invitation"] in state.invitations
        ],
    }


# --------------------------------------------------------------------------- checks


ORACLE_CHECKS = ("histories.records_valid", "histories.ordered", "histories.fold")


def found(check: str, message: str) -> str:
    if check not in ORACLE_CHECKS:
        raise ValueError(f"unregistered check {check}")
    return f"{check}: {message}"


def check_of(problem: str) -> str:
    return problem.split(":", 1)[0]


def as_draft(rec: dict, seq: int) -> dict:
    """The full control-stream draft a history record stands for, so §9.12's validator can judge it."""
    return {
        "envelope_version": 1,
        "environment": "paper",
        "event_id": f"01J8Z5H{seq:019d}",
        "stream_id": STREAM,
        "event_type": rec["event_type"],
        "schema_version": 1,
        "event_time": rec["event_time"],
        "clock_source": "local",
        "causation_id": None,
        "correlation_id": None,
        "actor": dict(rec["actor"]),
        "config_refs": {},
        "payload": copy.deepcopy(rec["payload"]),
        "artifact_refs": [],
        "pii_refs": [],
    }


def check_section(section: dict, skip: frozenset[str] = frozenset()) -> list[str]:
    problems = []
    for history in section["histories"]:
        name = history["name"]
        times = [instant_nanos(r["event_time"]) for r in history["records"]]
        if times != sorted(times):
            problems.append(found("histories.ordered", f"{name}: records out of time order"))
        for seq, rec in enumerate(history["records"], start=1):
            if rec["event_type"].startswith("Member"):
                got = violations(as_draft(rec, seq))
                if got:
                    problems.append(found("histories.records_valid", f"{name} record {seq}: {got}"))
        state = fold(history["records"], skip)
        if state.unreadable != history["unreadable"]:
            problems.append(found("histories.fold", f"{name}: unreadable {state.unreadable}"))
        for want in history["probes"]:
            at = instant_nanos(want["at"])
            so_far = fold([r for r in history["records"] if instant_nanos(r["event_time"]) <= at], skip)
            got = observed(so_far, want)
            if got != want:
                problems.append(found("histories.fold", f"{name} at {want['at']}: expected {want}, got {got}"))
    return problems


FOLD_MUTANTS = (
    "fold.invited_counts",
    "fold.cooling_counts",
    "fold.cool_off_ignored",
    "fold.per_role_cool_off_ignored",
    "fold.client_counted",
    "fold.removed_reactivates",
    "fold.unreadable_not_one",
    "fold.expiry_inclusive",
    "fold.old_roles_kept",
    "fold.deactivated_counts",
    "fold.revocation_ignored",
    "fold.revoke_any_state",
    "fold.revoke_unknown_tolerated",
    "fold.unknown_member_tolerated",
    "fold.duplicate_invitation_tolerated",
    "fold.second_activation_tolerated",
    "fold.invitation_reused",
    "fold.invitation_roles_unchecked",
    "fold.reactivation_roles_unchecked",
    "fold.unheld_role_removed",
    "fold.held_role_added",
    "fold.role_change_any_state",
    "fold.deactivate_any_state",
    "fold.remove_any_state",
    "fold.deactivated_removal_refused",
    "fold.deactivated_grant_accepted",
)


def vector_mutants(section: dict) -> list[tuple[str, str, dict]]:
    def mutated(fn) -> dict:
        out = copy.deepcopy(section)
        fn(out)
        return out

    def history(s, name):
        return next(h for h in s["histories"] if h["name"] == name)

    return [
        (
            "a history record breaks rule 106",
            "histories.records_valid",
            mutated(lambda s: history(s, "per_role_cool_off")["records"][3]["payload"].update(changed_at=ts(hours(9)))),
        ),
        (
            "two records swapped",
            "histories.ordered",
            mutated(lambda s: history(s, "per_role_cool_off")["records"].reverse()),
        ),
        (
            "an expected count typed wrong",
            "histories.fold",
            mutated(lambda s: history(s, "invited_cooling_then_active")["probes"][1].update(workspace_users=2)),
        ),
    ]


def run_mutants(section: dict) -> list[str]:
    escaped = []
    for mutant in FOLD_MUTANTS:
        if not any(check_of(p) == "histories.fold" for p in check_section(section, frozenset([mutant]))):
            escaped.append(f"membership fold mutant {mutant}")
    for check in ORACLE_CHECKS:
        if not any(c == check for _, c, _ in vector_mutants(section)):
            escaped.append(f"membership fold check {check} has no vector mutant registered against it")
    for name, check, mutated in vector_mutants(section):
        if check not in {check_of(p) for p in check_section(mutated)}:
            escaped.append(f"membership fold vector mutant: {name} (not caught by {check})")
    return escaped


def build_section() -> dict:
    return {"spec": SPEC, "histories": histories()}
