-- Requests to join the private beta, from the landing page's form (web/src/app/api/beta/route.ts).
-- The publishable key may add a row and never read one; read them in the dashboard or with the secret key.
create table if not exists public.beta_requests (
  id bigint generated always as identity primary key,
  email text not null check (char_length(email) between 3 and 254),
  role text check (role in ('own', 'clients', 'desk', 'builder')),
  created_at timestamptz not null default now()
);

create unique index if not exists beta_requests_email_key on public.beta_requests (lower(email));

alter table public.beta_requests enable row level security;

drop policy if exists "Anyone can ask to join" on public.beta_requests;
create policy "Anyone can ask to join" on public.beta_requests
  for insert to anon, authenticated
  with check (true);
