import { createSignal } from "solid-js";
import { api, ApiError, setToken } from "./api";

export function SignIn() {
  const [value, setValue] = createSignal("");
  const [error, setError] = createSignal<string | null>(null);
  const [busy, setBusy] = createSignal(false);

  const submit = async (e: Event) => {
    e.preventDefault();
    setBusy(true);
    setError(null);
    setToken(value().trim());
    try {
      await api.folders();
      location.reload();
    } catch (err) {
      setToken("");
      setError(err instanceof ApiError && err.status === 401 ? "That token isn't one of your devices." : "Can't reach bone.");
    } finally {
      setBusy(false);
    }
  };

  return (
    <main class="signin">
      <form onSubmit={submit}>
        <h1>bone</h1>
        <label for="token" class="meta">
          Device token
        </label>
        <input id="token" type="password" autocomplete="current-password" value={value()} onInput={(e) => setValue(e.currentTarget.value)} />
        {error() && <p class="error">{error()}</p>}
        <button class="btn primary" type="submit" disabled={busy() || !value().trim()}>
          Sign in
        </button>
      </form>
    </main>
  );
}
