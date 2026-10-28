-- The news reader's role writes only news streams (journal spec §6.1, DEC-2278 item 2). Applied as
-- mandate_journal_owner, after 0001. The role is a member of mandate_journal_app, so the grants
-- alone would let it append to any stream; these triggers refuse every row it writes outside
-- `news:`, as either the session's user or the current one, so a SET ROLE to the application role
-- does not lift the refusal. Every other role is unaffected.

CREATE FUNCTION journal_news_reader_writes_only_news() RETURNS trigger
LANGUAGE plpgsql SET search_path FROM CURRENT AS $$
BEGIN
    IF (session_user = 'mandate_news_reader' OR current_user = 'mandate_news_reader')
        AND NOT starts_with(NEW.stream_id, 'news:')
    THEN
        RAISE EXCEPTION 'mandate_news_reader writes only news streams, not %', NEW.stream_id
            USING ERRCODE = 'insufficient_privilege';
    END IF;
    RETURN NEW;
END
$$;

CREATE TRIGGER event_ids_news_reader_writes_only_news BEFORE INSERT ON event_ids
    FOR EACH ROW EXECUTE FUNCTION journal_news_reader_writes_only_news();
CREATE TRIGGER events_news_reader_writes_only_news BEFORE INSERT ON events
    FOR EACH ROW EXECUTE FUNCTION journal_news_reader_writes_only_news();
CREATE TRIGGER stream_heads_news_reader_writes_only_news BEFORE INSERT OR UPDATE ON stream_heads
    FOR EACH ROW EXECUTE FUNCTION journal_news_reader_writes_only_news();

REVOKE ALL ON FUNCTION journal_news_reader_writes_only_news() FROM PUBLIC;
