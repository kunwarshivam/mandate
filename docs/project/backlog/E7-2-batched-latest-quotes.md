- **E7-2zq (Proposed, M7; EX)** As an owner, I want my agents' instruments marked
  within the market-data host's request limit however many they hold, so that a large universe
  never reads its quotes too slowly to keep its marks fresh. Since DEC-2844 the executor marks
  each instrument every half of its quote staleness limit, 5 s for the reviewed 10 s, and
  `AlpacaQuotes` reads each instrument with its own `/v2/stocks/{symbol}/quotes/latest` request.
  The executor marks the distinct instruments of every deployment on the account, all read with
  the account's one data key. One deployment at its `max_instruments` ceiling of 20 then asks 240
  requests a minute for marks alone, over the 200 a minute DEC-2837 records for Alpaca, before the
  exit ladder's reads. Deployments a key serves are bounded only by the license's
  `max_agents_deployed`, which may be null; 20 deployments of 20 distinct instruments would ask
  4,800 a minute. A refused (`429`) read writes no mark, so the instrument goes stale (safe, but it
  holds openings, and a storm of them keeps the account's marks stale). Five instruments, DEC-117's
  default, ask 60 a minute, and the demo's one asks 12. One multi-symbol read a mark interval asks
  12 a minute whatever the count.
  *Accepted when:* the executor reads every instrument due a mark in one request
  (`/v2/stocks/quotes/latest?symbols=…&feed=iex`), or paces its marks to a recorded read budget
  without letting the interval reach the staleness limit, and a test shows 20 instruments marked
  at the DEC-2844 interval within that budget.
<!-- section: E7 Alpaca connector and recovery -->
