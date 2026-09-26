-- The journal hot store (journal spec §6.1, DEC-109). Applied only as mandate_journal_owner, which
-- owns every object here; mandate_journal_app must exist first. Functions pin the search_path of
-- the schema being migrated, so a caller's search_path cannot redirect them.

CREATE TABLE event_ids (
    event_id text PRIMARY KEY,
    stream_id text NOT NULL,
    seq bigint NOT NULL CHECK (seq >= 1),
    UNIQUE (stream_id, seq),
    UNIQUE (event_id, stream_id, seq)
);

CREATE TABLE events (
    stream_id text NOT NULL,
    seq bigint NOT NULL CHECK (seq >= 1),
    event_id text NOT NULL UNIQUE,
    event_type text NOT NULL,
    schema_version bigint NOT NULL CHECK (schema_version >= 1),
    environment text NOT NULL,
    recorded_at text NOT NULL,
    prev_hash bytea NOT NULL CHECK (octet_length(prev_hash) = 32),
    hash bytea NOT NULL,
    body bytea NOT NULL,
    PRIMARY KEY (stream_id, seq),
    CONSTRAINT events_hash_is_sha256_of_body CHECK (octet_length(hash) = 32 AND hash = sha256(body)),
    CONSTRAINT events_registered_in_event_ids FOREIGN KEY (event_id, stream_id, seq)
        REFERENCES event_ids (event_id, stream_id, seq) DEFERRABLE INITIALLY DEFERRED
);

-- risk_clock is canonical UTC text with nine fractional digits, so byte order is time order.
CREATE TABLE stream_heads (
    stream_id text PRIMARY KEY,
    seq bigint NOT NULL CHECK (seq >= 0),
    hash bytea NOT NULL CHECK (octet_length(hash) = 32),
    writer_epoch bigint NOT NULL CHECK (writer_epoch >= 0),
    risk_clock text COLLATE "C"
        CHECK (risk_clock ~ '^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}\.[0-9]{9}Z$')
);

CREATE FUNCTION journal_reject_change() RETURNS trigger
LANGUAGE plpgsql SET search_path FROM CURRENT AS $$
BEGIN
    RAISE EXCEPTION 'the journal is append-only: % on % is not allowed', TG_OP, TG_TABLE_NAME
        USING ERRCODE = 'restrict_violation';
END
$$;

CREATE FUNCTION journal_event_links() RETURNS trigger
LANGUAGE plpgsql SET search_path FROM CURRENT AS $$
DECLARE
    predecessor bytea;
BEGIN
    IF NEW.seq = 1 THEN
        predecessor := decode(repeat('00', 32), 'hex');
    ELSE
        SELECT hash INTO predecessor FROM events
        WHERE stream_id = NEW.stream_id AND seq = NEW.seq - 1;
    END IF;
    IF predecessor IS NULL OR NEW.prev_hash IS DISTINCT FROM predecessor THEN
        RAISE EXCEPTION 'event % of % does not follow its predecessor', NEW.seq, NEW.stream_id
            USING ERRCODE = 'restrict_violation';
    END IF;
    RETURN NEW;
END
$$;

CREATE FUNCTION journal_head_starts_empty() RETURNS trigger
LANGUAGE plpgsql SET search_path FROM CURRENT AS $$
BEGIN
    IF NEW.seq <> 0 OR NEW.hash <> decode(repeat('00', 32), 'hex') OR NEW.risk_clock IS NOT NULL THEN
        RAISE EXCEPTION 'a new head of % must be empty', NEW.stream_id
            USING ERRCODE = 'restrict_violation';
    END IF;
    RETURN NEW;
END
$$;

CREATE FUNCTION journal_head_advances() RETURNS trigger
LANGUAGE plpgsql SET search_path FROM CURRENT AS $$
BEGIN
    IF NEW.stream_id <> OLD.stream_id
        OR NEW.seq < OLD.seq
        OR NEW.writer_epoch < OLD.writer_epoch
        OR (OLD.risk_clock IS NOT NULL
            AND (NEW.risk_clock IS NULL OR NEW.risk_clock < OLD.risk_clock))
        OR (NEW.seq = 0 AND NEW.hash <> decode(repeat('00', 32), 'hex'))
        OR (NEW.seq > 0 AND NOT EXISTS (
            SELECT 1 FROM events
            WHERE stream_id = NEW.stream_id AND seq = NEW.seq AND hash = NEW.hash))
    THEN
        RAISE EXCEPTION 'the head of % may only advance to a stored event', OLD.stream_id
            USING ERRCODE = 'restrict_violation';
    END IF;
    RETURN NEW;
END
$$;

CREATE TRIGGER events_follow_their_predecessor BEFORE INSERT ON events
    FOR EACH ROW EXECUTE FUNCTION journal_event_links();
CREATE TRIGGER events_are_append_only BEFORE UPDATE OR DELETE OR TRUNCATE ON events
    FOR EACH STATEMENT EXECUTE FUNCTION journal_reject_change();
CREATE TRIGGER event_ids_are_append_only BEFORE UPDATE OR DELETE OR TRUNCATE ON event_ids
    FOR EACH STATEMENT EXECUTE FUNCTION journal_reject_change();
CREATE TRIGGER stream_heads_start_empty BEFORE INSERT ON stream_heads
    FOR EACH ROW EXECUTE FUNCTION journal_head_starts_empty();
CREATE TRIGGER stream_heads_only_advance BEFORE UPDATE ON stream_heads
    FOR EACH ROW EXECUTE FUNCTION journal_head_advances();
CREATE TRIGGER stream_heads_are_kept BEFORE DELETE OR TRUNCATE ON stream_heads
    FOR EACH STATEMENT EXECUTE FUNCTION journal_reject_change();

REVOKE ALL ON event_ids, events, stream_heads FROM PUBLIC;
REVOKE ALL ON FUNCTION journal_reject_change(), journal_event_links(), journal_head_starts_empty(),
    journal_head_advances() FROM PUBLIC;
GRANT SELECT, INSERT ON event_ids, events, stream_heads TO mandate_journal_app;
GRANT UPDATE (seq, hash, writer_epoch, risk_clock) ON stream_heads TO mandate_journal_app;
