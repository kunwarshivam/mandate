# E2-10 follow-up: the news reader folds its stream before it takes the writer epoch

- **E2-10d (Proposed, M5; lane SC)** As the founder, I want a news reader that cannot run to leave
  a healthy earlier reader writing, so that one bad start does not stop the workspace's news.
  Today the reader takes its writer epoch first and folds its stream second (DEC-2286). So a new
  reader that finds a stored item it cannot read has already fenced the earlier writer, and then
  stays not ready with `news_stream_unreadable`, so nothing writes the stream until someone
  intervenes. *Accepted when:* the reader reads and folds the stream's rows before
  `take_ownership`. A row it cannot read leaves it not ready, holding no epoch and having fenced
  nothing. Once it takes the epoch, it folds again from the head it read, since rows may have been
  appended between the two reads. A start test shows that an earlier writer's append still commits
  after a new reader refuses `news_stream_unreadable`. R3's note on #2893.
<!-- section: E2 Market data -->
