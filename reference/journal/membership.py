"""Journal spec v0.25 §9.12's reference vectors (DEC-437 item 9, DEC-648): the control stream's seven
membership records.

The schemas and rules 96 to 106 live in `control.py`, beside §9.2's to §9.7's, so one validator judges
every closed schema. This module builds the `membership` section: a base draft of each record, an
invalid draft for every member type and rule, and valid drafts for the cases a rule might be misread
to refuse. It checks the section with an oracle of its own: every cool-off a valid draft states is
recomputed with calendar arithmetic, independently of rule 103's nanosecond comparison. Every seeded
bug is shown caught.
"""

from __future__ import annotations

import copy
from datetime import datetime, timedelta

from common import change, delete, digest_strings
from control import (
    STREAM,
    check_of,
    draft_for,
    reported,
    violations,
)
from control import (
    invalid as control_invalid,
)
from control import (
    valid as control_valid,
)

SPEC = "docs/specs/journal.md v0.25 §9.12 (DEC-437 item 9, DEC-648)"
AT = "2026-10-05T14:00:00.000000000Z"
A_DAY_LATER = "2026-10-06T14:00:00.000000000Z"
EXPIRES = "2026-10-12T14:00:00.000000000Z"
ADMIN = "01J8Z4M0AD0000000000000AD1"
MEMBER = "01J8Z4M0BE0000000000000ME1"
INVITATION = "01J8Z4M0C00000000000000N01"
SERVICES = {"kind": "system", "id": "control_services", "version": "0.1.0", "build": "sha256:" + "d" * 64}
CLIENT = {"kind": "client", "id": "client_b_01", "version": "1", "build": None, "on_behalf_of": ADMIN}
AGENT = {"kind": "agent", "id": "agent_a", "version": "0.1.0", "build": "sha256:" + "c" * 64}
IDS = {
    "invited": "01J8Z4M1A000000000000000M1",
    "invitation_revoked": "01J8Z4M1B000000000000000M2",
    "activated": "01J8Z4M1C000000000000000M3",
    "founding": "01J8Z4M1D000000000000000M4",
    "role_changed": "01J8Z4M1E000000000000000M5",
    "deactivated": "01J8Z4M1F000000000000000M6",
    "reactivated": "01J8Z4M1G000000000000000M7",
    "removed": "01J8Z4M1H000000000000000M8",
}
STEP_UP = {"assertion_id": "assert_admin_01", "authenticated_at": "2026-10-05T13:59:30.000000000Z", "method": "passkey"}


def user(principal: str) -> dict:
    return {"kind": "user", "id": principal, "version": "1", "build": None}


def envelope(name: str, event_type: str, actor: dict, payload: dict) -> dict:
    return {
        "envelope_version": 1,
        "environment": "paper",
        "event_id": IDS[name],
        "stream_id": STREAM,
        "event_type": event_type,
        "schema_version": 1,
        "event_time": AT,
        "clock_source": "local",
        "causation_id": None,
        "correlation_id": None,
        "actor": dict(actor),
        "config_refs": {},
        "payload": payload | {"session_ref": f"session_{name}" if actor["kind"] == "user" else None},
        "artifact_refs": sorted(digest_strings(payload)),
        "pii_refs": [],
    }


def base_drafts() -> dict[str, dict]:
    return {
        "invited": envelope(
            "invited",
            "MemberInvited",
            user(ADMIN),
            {
                "invitation": INVITATION,
                "roles": ["approver", "viewer"],
                "invited_by": ADMIN,
                "step_up": dict(STEP_UP),
                "invited_at": AT,
                "expires_at": EXPIRES,
            },
        )
        | {"pii_refs": ["pii_invitee_address_01"]},
        "invitation_revoked": envelope(
            "invitation_revoked",
            "MemberInvitationRevoked",
            user(ADMIN),
            {"invitation": INVITATION, "revoked_by": ADMIN},
        ),
        "activated": envelope(
            "activated",
            "MemberActivated",
            user(MEMBER),
            {
                "member": MEMBER,
                "invitation": INVITATION,
                "reason": "invitation_accepted",
                "roles": ["approver", "viewer"],
                "method": "oidc",
                "activated_at": AT,
                "independent_approval_required": True,
                "cool_off_ends_at": A_DAY_LATER,
            },
        ),
        "founding": envelope(
            "founding",
            "MemberActivated",
            SERVICES,
            {
                "member": ADMIN,
                "invitation": None,
                "reason": "founding",
                "roles": ["approver", "operator", "workspace_admin"],
                "method": "passkey",
                "activated_at": AT,
                "independent_approval_required": False,
                "cool_off_ends_at": AT,
            },
        ),
        "role_changed": envelope(
            "role_changed",
            "MemberRoleChanged",
            user(ADMIN),
            {
                "member": MEMBER,
                "changed_by": ADMIN,
                "added": [
                    {"role": "auditor", "cool_off_ends_at": AT},
                    {"role": "operator", "cool_off_ends_at": A_DAY_LATER},
                ],
                "removed": ["viewer"],
                "changed_at": AT,
                "independent_approval_required": True,
                "step_up": dict(STEP_UP),
            },
        ),
        "deactivated": envelope(
            "deactivated",
            "MemberDeactivated",
            user(ADMIN),
            {"member": MEMBER, "by": ADMIN, "reason": "admin"},
        ),
        "reactivated": envelope(
            "reactivated",
            "MemberReactivated",
            user(ADMIN),
            {
                "member": MEMBER,
                "by": ADMIN,
                "step_up": dict(STEP_UP),
                "roles": ["approver", "viewer"],
                "reactivated_at": AT,
                "independent_approval_required": True,
                "cool_off_ends_at": A_DAY_LATER,
            },
        ),
        "removed": envelope(
            "removed",
            "MemberRemoved",
            user(ADMIN),
            {"member": MEMBER, "by": ADMIN, "reason": "admin"},
        ),
    }


BASES = base_drafts()


def draft_change(draft: dict, item: dict) -> None:
    *parents, last = item["path"].split(".")
    node = draft
    for name in parents:
        node = node[name]
    if item.get("delete"):
        del node[last]
    else:
        node[last] = copy.deepcopy(item["value"])


def invalid(name, clause, base, changes, reason, path, also=()):
    return control_invalid(name, clause, base, changes, reason, path, also)


def valid(name, clause, base, changes):
    return control_valid(name, clause, base, changes)


# Each member, a value of the wrong JSON kind (`schema`) and, where its type constrains a string, a
# string of the wrong form (`non_canonical`).
MEMBER_CASES = {
    "invited": (
        ("invitation", 7, "not-a-ulid"),
        ("roles", "viewer", None),
        ("invited_by", 7, ""),
        ("step_up", "passkey", None),
        ("invited_at", 1790000000, "2026-10-05"),
        ("expires_at", 1791000000, "2026-10-12"),
        ("session_ref", 7, ""),
    ),
    "invitation_revoked": (
        ("invitation", 7, "not-a-ulid"),
        ("revoked_by", 7, ""),
    ),
    "activated": (
        ("member", 7, "user_member_01"),
        ("invitation", 7, "not-a-ulid"),
        ("reason", 7, "accepted"),
        ("roles", "viewer", None),
        ("method", 7, "password"),
        ("activated_at", 1790000000, "2026-10-05T14:00:00Z"),
        ("independent_approval_required", "true", None),
        ("cool_off_ends_at", 1790086400, "2026-10-06T14:00:00Z"),
    ),
    "role_changed": (
        ("member", 7, "user_member_01"),
        ("changed_by", 7, ""),
        ("added", {"role": "operator"}, None),
        ("removed", "viewer", None),
        ("changed_at", 1790000000, "2026-10-05"),
        ("independent_approval_required", 1, None),
        ("step_up", "passkey", None),
    ),
    "deactivated": (
        ("member", 7, "user_member_01"),
        ("by", 7, ""),
        ("reason", 7, "fired"),
    ),
    "reactivated": (
        ("member", 7, "user_member_01"),
        ("by", 7, ""),
        ("step_up", "passkey", None),
        ("roles", "viewer", None),
        ("reactivated_at", 1790000000, "2026-10-05"),
        ("independent_approval_required", "false", None),
        ("cool_off_ends_at", 1790086400, "2026-10-06"),
    ),
    "removed": (
        ("member", 7, "user_member_01"),
        ("by", 7, ""),
        ("reason", 7, "erased"),
    ),
}
# Non-nullable members a `null` must not satisfy, each its own case.
# A `null` writer fails rule 97, a `null` cool-off end rule 103, a `null` expiry rule 104, and a `null` own
# instant rule 106, at the member's own path with the same reason, so such a draft is refused identically whether or not its type is checked first.
RULE_TYPED = (
    "by",
    "changed_by",
    "invited_by",
    "revoked_by",
    "cool_off_ends_at",
    "expires_at",
    "invited_at",
    "activated_at",
    "changed_at",
    "reactivated_at",
)
NULLED = {
    "invited": ("invitation", "roles", "invited_by", "step_up", "expires_at"),
    "invitation_revoked": ("invitation", "revoked_by"),
    "activated": ("member", "reason", "roles", "method", "activated_at", "independent_approval_required", "cool_off_ends_at"),
    "role_changed": ("member", "changed_by", "added", "removed", "changed_at", "independent_approval_required"),
    "deactivated": ("member", "by", "reason"),
    "reactivated": ("member", "by", "step_up", "roles", "reactivated_at", "independent_approval_required", "cool_off_ends_at"),
    "removed": ("member", "by", "reason"),
}


def member_drafts() -> list[dict]:
    out = []
    for base, cases in MEMBER_CASES.items():
        for member, wrong_kind, wrong_form in cases:
            path = f"payload.{member}"
            out.append(invalid(f"{base}.{member}.kind", "§9.12 types", base, [change(path, wrong_kind)], "schema", path))
            if wrong_form is not None:
                form = [change(path, wrong_form)]
                out.append(invalid(f"{base}.{member}.form", "§9.12 types", base, form, "non_canonical", path))
        for member in NULLED[base]:
            path = f"payload.{member}"
            out.append(invalid(f"{base}.{member}.null", "§9.12 types", base, [change(path, None)], "schema", path))
        some = MEMBER_CASES[base][0][0]
        out.append(invalid(f"{base}.missing", "§9.12 closed", base, [delete(f"payload.{some}")], "schema", f"payload.{some}"))
        out.append(invalid(f"{base}.extra", "§9.12 closed", base, [change("payload.note", "x")], "schema", "payload.note"))
    return out


def invalid_drafts() -> list[dict]:
    """Each draft breaks exactly one rule, or the rules its `also` lists; together they cover every
    §9.12 member type and rule."""
    return [
        *member_drafts(),
        invalid(
            "invited_role_unknown",
            "§9.12: `role` is a workspace role; an org role is not",
            "invited",
            [change("payload.roles", ["org_owner"])],
            "non_canonical",
            "payload.roles[0]",
        ),
        invalid(
            "added_role_missing_its_end",
            "§9.12: each added role is closed",
            "role_changed",
            [change("payload.added", [{"role": "auditor"}])],
            "schema",
            "payload.added[0].cool_off_ends_at",
        ),
        invalid(
            "step_up_time_as_seconds",
            "§9.12: step-up evidence is §9.2's, with a timestamp",
            "reactivated",
            [change("payload.step_up.authenticated_at", 1790000000)],
            "schema",
            "payload.step_up.authenticated_at",
        ),
        invalid(
            "invited_on_an_agent_stream",
            "§9: a control-stream record",
            "invited",
            [change("stream_id", "agent:ws_01J8Z2:agent_a")],
            "wrong_stream",
            "event_type",
        ),
        invalid(
            "invited_roles_unsorted",
            "rule 96",
            "invited",
            [change("payload.roles", ["viewer", "approver"])],
            "non_canonical",
            "payload.roles",
        ),
        invalid(
            "activated_roles_repeated",
            "rule 96: a repeat is refused as a swap is",
            "activated",
            [change("payload.roles", ["approver", "approver"])],
            "non_canonical",
            "payload.roles",
        ),
        invalid(
            "removed_unsorted",
            "rule 96",
            "role_changed",
            [change("payload.removed", ["viewer", "approver"])],
            "non_canonical",
            "payload.removed",
        ),
        invalid(
            "added_unsorted",
            "rule 96",
            "role_changed",
            [
                change(
                    "payload.added",
                    [{"role": "operator", "cool_off_ends_at": A_DAY_LATER}, {"role": "auditor", "cool_off_ends_at": AT}],
                )
            ],
            "non_canonical",
            "payload.added",
        ),
        invalid(
            "invited_by_another",
            "rule 97: the writer is the inviter",
            "invited",
            [change("payload.invited_by", MEMBER)],
            "schema",
            "payload.invited_by",
        ),
        invalid(
            "revoked_by_another",
            "rule 97",
            "invitation_revoked",
            [change("payload.revoked_by", MEMBER)],
            "schema",
            "payload.revoked_by",
        ),
        invalid(
            "removed_by_another",
            "rule 97",
            "removed",
            [change("payload.by", "01J8Z4M0AD0000000000000AD2")],
            "schema",
            "payload.by",
        ),
        invalid(
            "invited_by_the_system",
            "rule 98: an admin invites",
            "invited",
            [change("actor", SERVICES), change("payload.invited_by", SERVICES["id"]), change("payload.session_ref", None)],
            "schema",
            "actor.kind",
        ),
        invalid(
            "founding_by_a_user",
            "rule 98: the system issues the founding grant (ID-13)",
            "founding",
            [change("actor", user(ADMIN)), change("payload.session_ref", "session_founder")],
            "schema",
            "actor.kind",
        ),
        invalid(
            "accepted_by_the_system",
            "rule 98: the invitee accepts",
            "activated",
            [change("actor", SERVICES | {"id": MEMBER}), change("payload.session_ref", None)],
            "schema",
            "actor.kind",
        ),
        invalid(
            "deprovisioned_by_a_user",
            "rule 98: the directory sync deprovisions",
            "deactivated",
            [change("payload.reason", "deprovisioned")],
            "schema",
            "actor.kind",
        ),
        invalid(
            "org_deleted_by_a_user",
            "rule 98",
            "removed",
            [change("payload.reason", "org_deleted")],
            "schema",
            "actor.kind",
        ),
        invalid(
            "role_changed_by_a_platform_operator",
            "rule 98: platform staff never change a membership (ID-12)",
            "role_changed",
            [change("actor", {"kind": "platform_operator", "id": ADMIN, "version": "1", "build": None}), change("payload.session_ref", None)],
            "schema",
            "actor.kind",
        ),
        invalid(
            "group_removed_by_a_user",
            "rule 98: the identity provider's group removal is the system's",
            "deactivated",
            [change("payload.reason", "group_removed")],
            "schema",
            "actor.kind",
        ),
        invalid(
            "deactivated_by_a_client",
            "rule 98: a client never changes a membership (ID-11)",
            "deactivated",
            [change("actor", CLIENT), change("payload.by", CLIENT["id"]), change("payload.session_ref", None)],
            "schema",
            "actor.kind",
        ),
        invalid(
            "role_changed_by_an_agent",
            "rule 98: only a user or the system writes a membership record",
            "role_changed",
            [change("actor", AGENT), change("payload.changed_by", AGENT["id"]), change("payload.session_ref", None)],
            "schema",
            "actor.kind",
        ),
        invalid(
            "own_roles_changed",
            "rule 99: no admin changes their own roles (ID-13)",
            "role_changed",
            [change("payload.member", ADMIN)],
            "schema",
            "payload.changed_by",
        ),
        invalid(
            "own_reactivation",
            "rule 99",
            "reactivated",
            [change("payload.member", ADMIN)],
            "schema",
            "payload.by",
        ),
        invalid(
            "own_deactivation_as_admin",
            "rule 99: leaving is `left`",
            "deactivated",
            [change("payload.member", ADMIN)],
            "schema",
            "payload.by",
        ),
        invalid(
            "own_removal",
            "rule 99",
            "removed",
            [change("payload.member", ADMIN)],
            "schema",
            "payload.by",
        ),
        invalid(
            "left_by_another",
            "rule 99: only the member leaves",
            "deactivated",
            [change("payload.reason", "left")],
            "schema",
            "payload.by",
        ),
        invalid(
            "accepted_without_its_invitation",
            "rule 99",
            "activated",
            [change("payload.invitation", None)],
            "schema",
            "payload.invitation",
        ),
        invalid(
            "founding_with_an_invitation",
            "rule 99",
            "founding",
            [change("payload.invitation", INVITATION)],
            "schema",
            "payload.invitation",
        ),
        invalid(
            "accepted_for_another",
            "rule 99: the invitee who signed in is the member",
            "activated",
            [change("payload.member", ADMIN)],
            "schema",
            "payload.member",
        ),
        invalid(
            "invited_to_no_role",
            "rule 100",
            "invited",
            [change("payload.roles", [])],
            "schema",
            "payload.roles",
        ),
        invalid(
            "activated_with_no_role",
            "rule 100",
            "activated",
            [change("payload.roles", []), change("payload.cool_off_ends_at", AT)],
            "schema",
            "payload.roles",
        ),
        invalid(
            "founding_without_an_admin",
            "rule 100: a workspace always has an admin",
            "founding",
            [change("payload.roles", ["approver", "operator"])],
            "schema",
            "payload.roles",
        ),
        invalid(
            "role_change_changing_nothing",
            "rule 100",
            "role_changed",
            [change("payload.added", []), change("payload.removed", []), change("payload.step_up", None)],
            "schema",
            "payload.added",
        ),
        invalid(
            "role_added_and_removed",
            "rule 100",
            "role_changed",
            [change("payload.removed", ["auditor"])],
            "schema",
            "payload.removed",
        ),
        invalid(
            "grant_without_step_up",
            "rule 101: a grant needs step-up",
            "role_changed",
            [change("payload.step_up", None)],
            "schema",
            "payload.step_up",
        ),
        invalid(
            "removal_with_step_up",
            "rule 101: a removal carries none",
            "role_changed",
            [change("payload.added", [])],
            "schema",
            "payload.step_up",
        ),
        invalid(
            "live_step_up_by_cli_confirm",
            "rule 102: `live` takes a passkey only",
            "invited",
            [change("environment", "live"), change("payload.step_up.method", "cli_confirm")],
            "schema",
            "payload.step_up.method",
        ),
        invalid(
            "paper_step_up_by_password",
            "rule 102",
            "reactivated",
            [change("payload.step_up.method", "password")],
            "schema",
            "payload.step_up.method",
        ),
        invalid(
            "cool_off_of_an_hour",
            "rule 103: zero or exactly 24 hours",
            "activated",
            [change("payload.cool_off_ends_at", "2026-10-05T15:00:00.000000000Z")],
            "schema",
            "payload.cool_off_ends_at",
        ),
        invalid(
            "cool_off_a_nanosecond_short",
            "rule 103",
            "reactivated",
            [change("payload.cool_off_ends_at", "2026-10-06T13:59:59.999999999Z")],
            "schema",
            "payload.cool_off_ends_at",
        ),
        invalid(
            "cool_off_before_its_start",
            "rule 103",
            "reactivated",
            [change("payload.cool_off_ends_at", "2026-10-04T14:00:00.000000000Z")],
            "schema",
            "payload.cool_off_ends_at",
        ),
        invalid(
            "founding_cooling_off",
            "rule 103: the founding grant never has one",
            "founding",
            [change("payload.cool_off_ends_at", A_DAY_LATER)],
            "schema",
            "payload.cool_off_ends_at",
        ),
        invalid(
            "viewer_cooling_off",
            "rule 103: only operator and approver may cool off",
            "activated",
            [change("payload.roles", ["auditor", "viewer"])],
            "schema",
            "payload.cool_off_ends_at",
        ),
        invalid(
            "added_auditor_cooling_off",
            "rule 103, at the added role's own end",
            "role_changed",
            [change("payload.added", [{"role": "auditor", "cool_off_ends_at": A_DAY_LATER}])],
            "schema",
            "payload.added[0].cool_off_ends_at",
        ),
        invalid(
            "step_up_after_the_instant",
            "rule 102: evidence authenticated after the grant fails closed (mandate spec §6.1)",
            "role_changed",
            [change("payload.step_up.authenticated_at", "2026-10-05T14:00:00.000000001Z")],
            "schema",
            "payload.step_up.authenticated_at",
        ),
        invalid(
            "step_up_a_nanosecond_stale",
            "rule 102: 300 seconds and no more",
            "reactivated",
            [change("payload.step_up.authenticated_at", "2026-10-05T13:54:59.999999999Z")],
            "schema",
            "payload.step_up.authenticated_at",
        ),
        invalid(
            "invitation_of_six_days",
            "rule 104: 7 days exactly",
            "invited",
            [change("payload.expires_at", "2026-10-11T14:00:00.000000000Z")],
            "schema",
            "payload.expires_at",
        ),
        invalid(
            "user_without_a_session",
            "rule 105: a user acts through a session",
            "deactivated",
            [change("payload.session_ref", None)],
            "schema",
            "payload.session_ref",
        ),
        invalid(
            "system_with_a_session",
            "rule 105: the system writes without one",
            "founding",
            [change("payload.session_ref", "session_founding")],
            "schema",
            "payload.session_ref",
        ),
        invalid(
            "activated_in_the_past",
            "rules 103 and 106: the reviewer's probe, a grant dated back to escape its cool-off",
            "activated",
            [
                change("payload.activated_at", "2020-01-01T00:00:00.000000000Z"),
                change("payload.cool_off_ends_at", "2020-01-02T00:00:00.000000000Z"),
            ],
            "schema",
            "payload.cool_off_ends_at",
            also=[("schema", "payload.activated_at")],
        ),
        invalid(
            "changed_a_nanosecond_in_the_future",
            "rule 106",
            "role_changed",
            [change("payload.changed_at", "2026-10-05T14:00:00.000000001Z")],
            "schema",
            "payload.changed_at",
        ),
        invalid(
            "invited_a_nanosecond_early",
            "rule 106: a week from a self-chosen instant is still refused",
            "invited",
            [
                change("payload.invited_at", "2026-10-05T13:59:59.999999999Z"),
                change("payload.expires_at", "2026-10-12T13:59:59.999999999Z"),
            ],
            "schema",
            "payload.invited_at",
        ),
        invalid(
            "reactivated_in_the_past",
            "rule 106",
            "reactivated",
            [change("payload.reactivated_at", "2026-10-04T14:00:00.000000000Z")],
            "schema",
            "payload.reactivated_at",
        ),
        invalid(
            "cooling_without_independence",
            "rule 103: no day is owed when independence is not required",
            "activated",
            [change("payload.independent_approval_required", False)],
            "schema",
            "payload.cool_off_ends_at",
        ),
        invalid(
            "operator_at_once_under_independence",
            "rule 103: a day is owed",
            "role_changed",
            [
                change(
                    "payload.added",
                    [{"role": "auditor", "cool_off_ends_at": AT}, {"role": "operator", "cool_off_ends_at": AT}],
                )
            ],
            "schema",
            "payload.added[1].cool_off_ends_at",
        ),
        invalid(
            "approver_reactivated_at_once_under_independence",
            "rule 103",
            "reactivated",
            [change("payload.cool_off_ends_at", AT)],
            "schema",
            "payload.cool_off_ends_at",
        ),
        invalid(
            "reactivated_with_no_role",
            "rule 100",
            "reactivated",
            [change("payload.roles", []), change("payload.cool_off_ends_at", AT)],
            "schema",
            "payload.roles",
        ),
        invalid(
            "reactivated_roles_unsorted",
            "rule 96",
            "reactivated",
            [change("payload.roles", ["viewer", "approver"])],
            "non_canonical",
            "payload.roles",
        ),
        invalid(
            "rules_reported_in_order",
            "§9.1 order: rule 97 before rule 99 before rule 103",
            "role_changed",
            [
                change("payload.changed_by", MEMBER),
                change("payload.added", [{"role": "operator", "cool_off_ends_at": "2026-10-05T15:00:00.000000000Z"}]),
            ],
            "schema",
            "payload.changed_by",
            also=[("schema", "payload.changed_by"), ("schema", "payload.added[0].cool_off_ends_at")],
        ),
    ]


def valid_drafts() -> list[dict]:
    """Cases a rule might be misread to refuse; each must be accepted."""
    return [
        valid(
            "activated_without_a_cool_off",
            "rule 103: without independence required, an approver activates at once",
            "activated",
            [change("payload.independent_approval_required", False), change("payload.cool_off_ends_at", AT)],
        ),
        valid(
            "viewer_activated_at_once",
            "rule 103: a role that never cools off",
            "activated",
            [change("payload.roles", ["auditor", "viewer"]), change("payload.cool_off_ends_at", AT)],
        ),
        valid(
            "founding_owner_bundle_in_live",
            "rules 98 to 100 and 103: the retail owner bundle (identity spec §4.3)",
            "founding",
            [change("environment", "live")],
        ),
        valid(
            "removal_only",
            "rules 100 and 101: removing a role needs no step-up",
            "role_changed",
            [change("payload.added", []), change("payload.step_up", None)],
        ),
        valid(
            "grant_only",
            "rule 100: nothing removed",
            "role_changed",
            [change("payload.removed", [])],
        ),
        valid(
            "operator_granted_without_a_cool_off",
            "rule 103: without independence required, an operator grant is effective at once",
            "role_changed",
            [
                change("payload.added", [{"role": "operator", "cool_off_ends_at": AT}]),
                change("payload.removed", []),
                change("payload.independent_approval_required", False),
            ],
        ),
        valid(
            "admin_granted_at_once_under_independence",
            "rule 103: `workspace_admin` never cools off, even where independence is required",
            "role_changed",
            [change("payload.added", [{"role": "workspace_admin", "cool_off_ends_at": AT}]), change("payload.removed", [])],
        ),
        valid(
            "left",
            "rules 98 and 99: a member leaves by themselves",
            "deactivated",
            [change("actor", user(MEMBER)), change("payload.by", MEMBER), change("payload.reason", "left")],
        ),
        valid(
            "deprovisioned_by_the_directory",
            "rule 98",
            "deactivated",
            [
                change("actor", SERVICES),
                change("payload.by", SERVICES["id"]),
                change("payload.reason", "deprovisioned"),
                change("payload.session_ref", None),
            ],
        ),
        valid(
            "group_removed",
            "rule 98",
            "deactivated",
            [
                change("actor", SERVICES),
                change("payload.by", SERVICES["id"]),
                change("payload.reason", "group_removed"),
                change("payload.session_ref", None),
            ],
        ),
        valid(
            "removed_by_org_deletion",
            "rule 98",
            "removed",
            [
                change("actor", SERVICES),
                change("payload.by", SERVICES["id"]),
                change("payload.reason", "org_deleted"),
                change("payload.session_ref", None),
            ],
        ),
        valid(
            "reactivated_at_once",
            "rule 103: without independence required",
            "reactivated",
            [change("payload.independent_approval_required", False), change("payload.cool_off_ends_at", AT)],
        ),
        valid(
            "viewer_reactivated_at_once_under_independence",
            "rule 103: kept roles that never cool off",
            "reactivated",
            [change("payload.roles", ["auditor", "viewer"]), change("payload.cool_off_ends_at", AT)],
        ),
        valid(
            "founding_under_independence",
            "rule 103: the founding grant never cools off, even where independence is required",
            "founding",
            [change("payload.independent_approval_required", True)],
        ),
        valid(
            "paper_step_up_by_cli_confirm",
            "rule 102: `cli_confirm` in paper (DEC-155)",
            "invited",
            [change("payload.step_up.method", "cli_confirm")],
        ),
        valid(
            "step_up_at_the_window_edge",
            "rule 102: exactly 300 seconds is still valid",
            "invited",
            [change("payload.step_up.authenticated_at", "2026-10-05T13:55:00.000000000Z")],
        ),
        valid(
            "step_up_at_the_instant",
            "rule 102: zero seconds is valid",
            "reactivated",
            [change("payload.step_up.authenticated_at", AT)],
        ),
        valid(
            "invited_to_every_role",
            "rule 96",
            "invited",
            [change("payload.roles", ["approver", "auditor", "operator", "viewer", "workspace_admin"])],
        ),
    ]


# --------------------------------------------------------------------------- independent oracles

ORACLE_CHECKS = ("drafts.valid", "drafts.cool_off", "drafts.timing", "invalid_drafts", "valid_drafts")
COOLING = ("approver", "operator")


def found(check: str, message: str) -> str:
    if check not in ORACLE_CHECKS:
        raise ValueError(f"unregistered check {check}")
    return f"{check}: {message}"


def instant(text: str) -> datetime:
    return datetime.strptime(text, "%Y-%m-%dT%H:%M:%S.%f000Z")


def stated_cool_offs(draft: dict) -> list[tuple[str, bool]]:
    """Identity spec §8.3, read from the record by its own path: each cool-off's end, and whether a
    day is owed: independence required and operator or approver added to an existing workspace."""
    p = draft["payload"]
    kind = draft["event_type"]
    independent = p.get("independent_approval_required") is True
    if kind == "MemberActivated":
        founding = p["reason"] == "founding"
        return [(p["cool_off_ends_at"], independent and not founding and bool(set(p["roles"]) & set(COOLING)))]
    if kind == "MemberReactivated":
        return [(p["cool_off_ends_at"], independent and bool(set(p["roles"]) & set(COOLING)))]
    if kind == "MemberRoleChanged":
        return [(a["cool_off_ends_at"], independent and a["role"] in COOLING) for a in p["added"]]
    return []


OWN_INSTANT = {
    "MemberInvited": "invited_at",
    "MemberActivated": "activated_at",
    "MemberRoleChanged": "changed_at",
    "MemberReactivated": "reactivated_at",
}


def timing_problems(name: str, draft: dict) -> list[str]:
    """Mandate spec §6.1's step-up window and identity spec §5.2's 7-day invitation, recomputed with
    calendar arithmetic."""
    p = draft["payload"]
    out = []
    step_up = p.get("step_up")
    if step_up is not None:
        at = instant(draft["event_time"])
        authenticated = instant(step_up["authenticated_at"])
        if not authenticated <= at <= authenticated + timedelta(minutes=5):
            out.append(found("drafts.timing", f"{name}: step-up at {step_up['authenticated_at']} is not valid then"))
    if draft["event_type"] == "MemberInvited" and instant(p["expires_at"]) != instant(p["invited_at"]) + timedelta(days=7):
        out.append(found("drafts.timing", f"{name}: an invitation that does not last 7 days"))
    own = OWN_INSTANT.get(draft["event_type"])
    if own and instant(p[own]) != instant(draft["event_time"]):
        out.append(found("drafts.timing", f"{name}: {own} is not the envelope's event_time"))
    return out


def cool_off_problems(name: str, draft: dict) -> list[str]:
    out = []
    start = instant(draft["event_time"])
    for end, owed in stated_cool_offs(draft):
        if instant(end) != (start + timedelta(days=1) if owed else start):
            out.append(found("drafts.cool_off", f"{name}: a cool-off ending {end}"))
    return out


def check_section(section: dict) -> list[str]:
    """Every failure, so seeded bugs can be shown caught."""
    problems = []
    for name, draft in section["drafts"].items():
        got = violations(draft)
        if got:
            problems.append(found("drafts.valid", f"base draft {name}: {got}"))
        problems += cool_off_problems(name, draft)
        problems += timing_problems(name, draft)
    for case in section["invalid_drafts"]:
        got = violations(draft_for(section, case))
        if not reported(got, case["expect"]):
            problems.append(found("invalid_drafts", f"{case['name']}: expected {case['expect']}, got {got}"))
    for case in section["valid_drafts"]:
        draft = draft_for(section, case)
        got = violations(draft)
        if got:
            problems.append(found("valid_drafts", f"{case['name']}: expected Valid, got {got}"))
        problems += cool_off_problems(case["name"], draft)
        problems += timing_problems(case["name"], draft)
    return problems


# --------------------------------------------------------------------------- seeded bugs

VALIDATOR_MUTANTS = (
    "rule.96.roles",
    "rule.96.removed",
    "rule.96.added",
    "rule.97",
    "rule.98",
    "rule.99.self",
    "rule.99.left",
    "rule.99.invitation",
    "rule.99.invitee",
    "rule.100.empty",
    "rule.100.founding_admin",
    "rule.100.no_change",
    "rule.100.disjoint",
    "rule.101",
    "rule.102.method",
    "rule.102.window",
    "rule.103.MemberActivated",
    "rule.103.MemberReactivated",
    "rule.103.MemberRoleChanged",
    "rule.104",
    "rule.105",
    "boundary.rule_102_after",
    "boundary.rule_102_window",
    "rule.106",
    "boundary.rule_103_ignores_independence",
    "boundary.rule_103_founding_cools",
    "boundary.rule_103_either",
    "boundary.rule_103_admin_cools",
    "record.extra",
    "record.missing",
    *sorted({f"loose.payload.{member}" for cases in MEMBER_CASES.values() for member, _, _ in cases}),
    *sorted(
        {f"nullable.payload.{member}" for members in NULLED.values() for member in members}
        - {f"nullable.payload.{member}" for member in RULE_TYPED}
    ),
)


def vector_mutants(section: dict) -> list[tuple[str, str, dict]]:
    """Seeded bugs in the vectors, each registered against the one check that must reject it."""

    def mutated(fn) -> dict:
        out = copy.deepcopy(section)
        fn(out)
        return out

    def case(s, kind, name):
        return next(c for c in s[kind] if c["name"] == name)

    return [
        (
            "a base draft breaks rule 97",
            "drafts.valid",
            mutated(lambda s: s["drafts"]["removed"]["payload"].update(by=MEMBER)),
        ),
        (
            "a valid viewer activation cools off for a day",
            "drafts.cool_off",
            mutated(
                lambda s: case(s, "valid_drafts", "viewer_activated_at_once")["changes"].append(
                    change("payload.cool_off_ends_at", A_DAY_LATER)
                )
            ),
        ),
        (
            "an invalid draft's expectation differs",
            "invalid_drafts",
            mutated(lambda s: case(s, "invalid_drafts", "grant_without_step_up")["expect"].update(path="payload.added")),
        ),
        (
            "a valid draft's step-up is a second past its window",
            "drafts.timing",
            mutated(
                lambda s: case(s, "valid_drafts", "step_up_at_the_window_edge")["changes"].append(
                    change("payload.step_up.authenticated_at", "2026-10-05T13:54:59.000000000Z")
                )
            ),
        ),
        (
            "a valid draft breaks a rule",
            "valid_drafts",
            mutated(lambda s: case(s, "valid_drafts", "grant_only")["changes"].append(change("payload.step_up", None))),
        ),
    ]


def run_mutants(section: dict) -> list[str]:
    """Every seeded bug caught; a vector mutant only by the check it is registered against."""
    escaped = []
    cases = [(copy.deepcopy(d), None) for d in section["drafts"].values()]
    cases += [(draft_for(section, c), None) for c in section["valid_drafts"]]
    cases += [(draft_for(section, c), c["expect"]) for c in section["invalid_drafts"]]
    for mutant in VALIDATOR_MUTANTS:
        skip = frozenset([mutant])
        caught = False
        for draft, want in cases:
            got = violations(draft, skip)
            caught |= bool(got) if want is None else not reported(got, want)
        if not caught:
            escaped.append(f"membership validator mutant {mutant}")
    registered = vector_mutants(section)
    for check in ORACLE_CHECKS:
        if not any(c == check for _, c, _ in registered):
            escaped.append(f"membership check {check} has no vector mutant registered against it")
    for name, check, mutated in registered:
        caught_by = {check_of(problem) for problem in check_section(mutated)}
        if check not in caught_by:
            escaped.append(f"membership vector mutant: {name} (not caught by {check}; caught by {sorted(caught_by)})")
    return escaped


# --------------------------------------------------------------------------- output


def build_section() -> dict:
    return {
        "spec": SPEC,
        "drafts": base_drafts(),
        "invalid_drafts": invalid_drafts(),
        "valid_drafts": valid_drafts(),
    }
