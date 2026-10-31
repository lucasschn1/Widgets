"""Widget do GitHub: mapa de contribuições e atividades recentes."""

import json
import math
import re
import shutil
import subprocess
import textwrap
import threading
import time
import urllib.parse
import urllib.request
import webbrowser
from datetime import date, datetime, timedelta, timezone

from deskwidgets.theme import DIVIDER, MUTED, translucent
from deskwidgets.ui.card import CardWidget
from deskwidgets.util import MONTHS, WEEKDAYS, time_ago


# ---------- widget: github ----------

GITHUB_GREEN = (63, 185, 80, 255)
# níveis do mapa de calor, nas cores do tema escuro do GitHub
HEAT = [translucent((255, 255, 255), 0.10), (20, 88, 50, 255), (0, 109, 50, 255),
        (38, 166, 65, 255), (57, 211, 83, 255)]
KIND_COLOR = {
    "commit": (88, 166, 255), "pr_open": (63, 185, 80), "pr_merged": (163, 113, 247),
    "pr_closed": (248, 81, 73), "branch": (176, 186, 200), "delete": (248, 81, 73),
    "star": (227, 179, 65), "issue": (63, 185, 80), "comment": (176, 186, 200),
    "tag": (88, 166, 255), "generic": (176, 186, 200),
}
PR_STATE = {"opened": "aberto", "closed": "fechado", "merged": "mesclado", "reopened": "reaberto"}


def gh_token():
    """Token do GitHub CLI (`gh`), se ele estiver instalado e logado; libera os repositórios privados."""
    exe = shutil.which("gh")
    if not exe:
        return None
    try:
        out = subprocess.run([exe, "auth", "token"], capture_output=True, text=True, timeout=10,
                             creationflags=0x08000000)  # CREATE_NO_WINDOW: sem piscar um console
    except (OSError, subprocess.SubprocessError):
        return None
    return out.stdout.strip() or None


def github_api(path, token):
    headers = {"User-Agent": "desktop-widget", "Accept": "application/vnd.github+json"}
    if token:
        headers["Authorization"] = f"Bearer {token}"
    req = urllib.request.Request("https://api.github.com" + path, headers=headers)
    with urllib.request.urlopen(req, timeout=15) as resp:
        return json.load(resp)


def contributions(user):
    """Dias do mapa de contribuições do último ano: [(data, nível 0–4, quantidade)] e o total."""
    url = f"https://github.com/users/{urllib.parse.quote(user)}/contributions"
    req = urllib.request.Request(url, headers={"User-Agent": "desktop-widget"})
    try:
        with urllib.request.urlopen(req, timeout=15) as resp:
            html = resp.read().decode("utf-8", "replace")
    except urllib.error.HTTPError as exc:
        if exc.code == 404:
            raise LookupError(f"Usuário não encontrado: {user}") from exc
        raise
    # a quantidade de cada dia está no texto do tooltip ligado à célula ("3 contributions on…")
    tips = dict(re.findall(r'<tool-tip[^>]*\bfor="([^"]+)"[^>]*>([^<]*)</tool-tip>', html))
    days = []
    for tag in re.findall(r'<td\b[^>]*\bdata-date="[^"]*"[^>]*>', html):
        attrs = dict(re.findall(r'([\w-]+)="([^"]*)"', tag))
        count = re.match(r"\s*(\d+)", tips.get(attrs.get("id"), ""))
        days.append((date.fromisoformat(attrs["data-date"]), int(attrs.get("data-level") or 0),
                     int(count.group(1)) if count else 0))
    if not days:
        raise ValueError("A página do GitHub mudou de formato")
    days.sort()
    total = re.search(r"([\d.,]+)\s+contributions?\s+in the last year", html)
    total = int(re.sub(r"\D", "", total.group(1))) if total else sum(c for _, _, c in days)
    return days, total


def describe_event(event, user, lookup):
    """Texto, ícone e link de um evento da API do GitHub."""
    kind, p = event["type"], event.get("payload") or {}
    repo = event["repo"]["name"]
    short = repo.split("/", 1)[1] if repo.lower().startswith(user.lower() + "/") else repo
    url = f"https://github.com/{repo}"
    info = short
    if kind == "PushEvent":
        branch = p.get("ref", "").removeprefix("refs/heads/")
        head = p.get("head")
        # a API deixou de mandar os commits no evento; o texto vem do commit de cima
        message = ((p.get("commits") or [{}])[-1].get("message")
                   or (lookup(f"/repos/{repo}/commits/{head}") or {}).get("commit", {}).get("message"))
        title = message.splitlines()[0] if message else f"Push em {branch}"
        icon, info = "commit", f"{short}  ·  {branch}"
        if head:
            url += f"/commit/{head}"
    elif kind == "PullRequestEvent":
        number, pr = p.get("number"), p.get("pull_request") or {}
        action = "merged" if p.get("action") == "closed" and pr.get("merged") else p.get("action", "")
        title = (pr.get("title") or (lookup(f"/repos/{repo}/pulls/{number}") or {}).get("title")
                 or f"Pull request #{number}")
        icon = {"merged": "pr_merged", "closed": "pr_closed"}.get(action, "pr_open")
        info = f"{short}  ·  #{number} {PR_STATE.get(action, action)}"
        url += f"/pull/{number}"
    elif kind in ("CreateEvent", "DeleteEvent"):
        ref, ref_type = p.get("ref") or "", p.get("ref_type")
        verb = "Criou" if kind == "CreateEvent" else "Apagou"
        if ref_type == "repository":
            title, icon = f"{verb} o repositório", "generic"
        elif ref_type == "tag":
            title, icon = f"{verb} a tag {ref}", "tag"
        else:
            title, icon = f"{verb} o branch {ref}", "branch" if kind == "CreateEvent" else "delete"
            if kind == "CreateEvent":
                url += f"/tree/{ref}"
    elif kind == "WatchEvent":
        title, icon, info = f"Deu estrela em {repo}", "star", ""
    elif kind == "ForkEvent":
        title, icon, info = f"Fez fork de {repo}", "branch", ""
    elif kind in ("IssuesEvent", "IssueCommentEvent"):
        issue = p.get("issue") or {}
        number = issue.get("number") or p.get("number")
        text = issue.get("title") or f"#{number}"
        if kind == "IssuesEvent":
            title, icon = text, "issue"
            info = f"{short}  ·  issue {'fechada' if p.get('action') == 'closed' else 'aberta'}"
        else:
            title, icon = f"Comentou em {text}", "comment"
        url += f"/issues/{number}"
    elif kind in ("PullRequestReviewEvent", "PullRequestReviewCommentEvent"):
        number = (p.get("pull_request") or {}).get("number") or p.get("number")
        title, icon = f"Revisou o PR #{number}", "comment"
        url += f"/pull/{number}"
    elif kind == "ReleaseEvent":
        release = p.get("release") or {}
        title, icon = f"Publicou {release.get('name') or release.get('tag_name') or 'uma versão'}", "tag"
    elif kind == "PublicEvent":
        title, icon = "Tornou o repositório público", "generic"
    else:
        title, icon = kind.removesuffix("Event"), "generic"
    when = datetime.fromisoformat(event["created_at"].replace("Z", "+00:00"))
    return {"title": title, "info": info, "icon": icon, "when": when, "url": url}


def draw_glyph(d, kind, cx, cy, size, color, bg):
    """Ícone de atividade no estilo dos Octicons, num quadro de lado `size` centrado em (cx, cy)."""
    u = size / 2
    w = max(1, round(size * 0.12))

    def pt(x, y):
        return cx + x * u, cy + y * u

    def line(*points):
        pts = [pt(x, y) for x, y in points]
        d.line(pts, fill=color, width=w, joint="curve")
        for x, y in (pts[0], pts[-1]):
            d.ellipse((x - w / 2, y - w / 2, x + w / 2, y + w / 2), fill=color)

    def ring(x, y, r=0.27):
        x, y = pt(x, y)
        d.ellipse((x - r * u, y - r * u, x + r * u, y + r * u), fill=bg, outline=color, width=w)

    if kind == "commit":
        line((-1, 0), (1, 0))
        ring(0, 0, 0.45)
    elif kind in ("pr_open", "pr_closed"):
        line((-0.5, -0.62), (-0.5, 0.62))
        ring(-0.5, -0.62)
        ring(-0.5, 0.62)
        line((0.5, 0.6), (0.5, -0.45), (0.05, -0.45))
        line((0.05, -0.45), (0.27, -0.67))
        line((0.05, -0.45), (0.27, -0.23))
        ring(0.5, 0.62)
    elif kind == "pr_merged":
        line((-0.5, -0.62), (-0.5, 0.62))
        line((-0.5, -0.4), (-0.2, 0.2), (0.5, 0.2))
        ring(-0.5, -0.62)
        ring(-0.5, 0.62)
        ring(0.5, 0.2)
    elif kind in ("branch", "delete"):
        line((-0.5, -0.62), (-0.5, 0.62))
        line((0.5, -0.4), (0.5, -0.1), (-0.5, 0.35))
        ring(-0.5, -0.62)
        ring(-0.5, 0.62)
        ring(0.5, -0.55)
    elif kind == "star":
        pts = [pt(math.cos(math.radians(-90 + i * 36)) * (0.95 if i % 2 == 0 else 0.42),
                  math.sin(math.radians(-90 + i * 36)) * (0.95 if i % 2 == 0 else 0.42) + 0.06)
               for i in range(10)]
        d.polygon(pts, fill=color)
    elif kind == "issue":
        ring(0, 0, 0.8)
        x, y = pt(0, 0)
        d.ellipse((x - 0.2 * u, y - 0.2 * u, x + 0.2 * u, y + 0.2 * u), fill=color)
    elif kind == "comment":
        (x0, y0), (x1, y1) = pt(-0.85, -0.7), pt(0.85, 0.45)
        d.rounded_rectangle((x0, y0, x1, y1), radius=0.3 * u, outline=color, width=w)
        line((-0.35, 0.45), (-0.55, 0.85), (0.05, 0.45))
    elif kind == "tag":
        d.polygon([pt(-0.8, -0.8), pt(0.1, -0.8), pt(0.85, 0), pt(0.1, 0.8), pt(-0.8, 0.8)],
                  outline=color, width=w)
        ring(-0.38, -0.38, 0.16)
    else:
        ring(0, 0, 0.45)


def github_to_cache(data):
    return {**data, "days": [[d.isoformat(), level, count] for d, level, count in data["days"]],
            "rows": [{**row, "when": row["when"].isoformat()} for row in data["rows"]]}


def github_from_cache(data):
    try:
        return {**data, "days": [(date.fromisoformat(d), level, count) for d, level, count in data["days"]],
                "rows": [{**row, "when": datetime.fromisoformat(row["when"])} for row in data["rows"]]}
    except (KeyError, TypeError, ValueError):
        return None


class GitHubWidget(CardWidget):
    key = "github"
    title = "GitHub"
    description = "Mapa de contribuições, sequência de dias e atividades recentes."
    icon = "github"

    W, PAD, RADIUS = 320, 18, 26
    LIST_Y, ROW_H, ROWS = 92, 40, 6
    H = LIST_Y + ROW_H * ROWS + 8
    MAX_ROWS = 20  # carregadas; a roda do mouse rola a lista
    ICON_D = 26

    # pequeno, com a mesma largura dos outros: os últimos 6 meses, e a altura que der
    COMPACT_W, COMPACT_RADIUS = W, 22
    WEEKS, GAP, C_PAD = 26, 2, 14
    STEP = (COMPACT_W - 2 * C_PAD + GAP) / WEEKS
    CELL = STEP - GAP
    COMPACT_H = round(2 * C_PAD + 7 * STEP - GAP)
    GRID_X = C_PAD
    GRID_Y = (COMPACT_H - (7 * STEP - GAP)) / 2

    # médio: o mapa e as 3 últimas atividades
    M_LIST_Y, M_ROWS = COMPACT_H + 6, 3
    MEDIUM_H = M_LIST_Y + ROW_H * M_ROWS + 2

    FETCH_EVERY_MS = 10 * 60 * 1000
    RETRY_MS = 2 * 60 * 1000
    CLOCK_MS = 60 * 1000  # atualiza o "há 5 min"

    def __init__(self, app):
        cached = app.cache.get("github")
        user = app.cfg[self.key].get("usuario")
        # abre com os últimos dados guardados, enquanto busca os atuais
        self.data = github_from_cache(cached) if cached and cached.get("user") == user else None
        self.msg = "Carregando…"
        self._result = None
        self._busy = False
        self._job = None
        self._token = None
        self._lookups = {}  # respostas da API que não mudam (mensagem de commit, título de PR)
        self.scroll = 0
        super().__init__(app)
        self.updated = app.cache.get("github_time") if self.data else None
        self.refresh()
        self.send_to_back()
        self.update_data()
        self.after(self.CLOCK_MS, self._clock)

    def default_pos(self, w, h):
        # embaixo do widget mais baixo, encaixado nele
        others = self.others()
        if others:
            return self.below(max(others, key=lambda wd: wd.card_rect()[3]))
        return super().default_pos(w, h)

    def layout(self, mode):
        if mode == "p":
            return {"w": self.COMPACT_W, "h": self.COMPACT_H, "radius": self.COMPACT_RADIUS}
        if mode == "m":
            return {"w": self.W, "h": self.MEDIUM_H, "radius": self.RADIUS}
        return {"w": self.W, "h": self.H, "radius": self.RADIUS}

    def _list_geom(self, mode=None):
        """(topo da lista, linhas visíveis) no tamanho dado; None se não há lista."""
        mode = mode or self.mode
        return {"p": None, "m": (self.M_LIST_Y, self.M_ROWS), "g": (self.LIST_Y, self.ROWS)}[mode]

    # --- dados ---

    def update_data(self):
        if self._job:
            self.app.root.after_cancel(self._job)
            self._job = None
        if self._busy:
            return
        self._busy = self.loading = True
        self.present()
        reviews = self.app.notify_enabled("github")
        threading.Thread(target=self._worker, args=(self.cfg.get("usuario"), reviews), daemon=True).start()
        self.after(200, self._poll)

    def _worker(self, user, want_reviews):
        try:
            if self._token is None:
                self._token = gh_token() or self.cfg.get("token") or ""
            token = self._token
            if not user:
                if not token:
                    raise PermissionError
                user = github_api("/user", token)["login"]
            days, total = contributions(user)
            # com o token, a lista inclui os eventos dos repositórios privados do próprio usuário
            path = f"/users/{user}/events" + ("" if token else "/public") + "?per_page=60"
            events = sorted(github_api(path, token), key=lambda e: e["created_at"], reverse=True)

            def lookup(api_path):
                if api_path not in self._lookups:
                    try:
                        self._lookups[api_path] = github_api(api_path, token)
                    except Exception:
                        return None  # tenta de novo na próxima atualização
                return self._lookups[api_path]

            rows, seen = [], set()
            for e in events:
                p = e.get("payload") or {}
                # de cada PR ou issue, só o evento mais recente (ex.: "mesclado" no lugar de "aberto")
                grouped = e["type"] in ("PullRequestEvent", "IssuesEvent")
                key = (e["type"], e["repo"]["name"], None if grouped else p.get("action"),
                       p.get("number") or (p.get("issue") or {}).get("number"), p.get("ref"), p.get("head"))
                if key in seen:
                    continue
                seen.add(key)
                rows.append(describe_event(e, user, lookup))
                # sem token o limite da API é baixo (60 pedidos por hora): carrega menos
                if len(rows) == (self.MAX_ROWS if token else 10):
                    break
            reviews = None
            if want_reviews and token:
                query = urllib.parse.quote(f"is:pr is:open review-requested:{user}")
                items = github_api(f"/search/issues?q={query}&per_page=20", token).get("items", [])
                reviews = [{"id": i["id"], "title": i["title"], "url": i["html_url"],
                            "repo": i["repository_url"].rsplit("/", 1)[-1]} for i in items]
            self._result = ({"user": user, "days": days, "total": total, "rows": rows}, reviews, None)
        except Exception as exc:
            self._result = (None, None, exc)

    def _poll(self):
        if self._result is None:
            self.after(200, self._poll)
            return
        data, reviews, error = self._result
        self._result = None
        self._busy = self.loading = False
        delay = self.FETCH_EVERY_MS
        if isinstance(error, PermissionError):
            self.msg = "Clique direito → Mudar usuário"
            self.after(300, self.ask_user)
        elif isinstance(error, LookupError):
            self.data, self.msg, self.error = None, str(error), None
        elif error:
            self.msg = "Sem conexão. Clique para tentar de novo"
            self.error = "Sem conexão com o GitHub" if isinstance(error, OSError) else str(error)
            delay = self.RETRY_MS
        else:
            self.data, self.error, self.updated = data, None, time.time()
            self.scroll = min(self.scroll, max(0, len(data["rows"]) - self.ROWS))
            if self.cfg.get("usuario") != data["user"]:
                self.cfg["usuario"] = data["user"]
                self.save()
            self.app.cache.update(github=github_to_cache(data), github_time=self.updated)
            if reviews is not None:
                self._notify_reviews(reviews)
            self.app.save_cache()
        self.refresh()
        self._job = self.after(delay, self.update_data)

    def _notify_reviews(self, reviews):
        """Avisa dos pedidos de revisão novos (os que já existiam na primeira vez só são guardados)."""
        seen = self.app.cache.get("reviews_seen")
        ids = [r["id"] for r in reviews]
        if seen is not None:
            for review in reviews:
                if review["id"] not in seen:
                    self.app.notify("github", "Revisão pedida no GitHub",
                                    f"{review['title']} ({review['repo']})",
                                    action=lambda url=review["url"]: webbrowser.open(url))
        self.app.cache["reviews_seen"] = sorted(set(ids) | set(seen or []))[-200:]

    def _clock(self):
        if self.data and self.mode != "p":
            self.refresh()
        self.after(self.CLOCK_MS, self._clock)

    def ask_user(self):
        self.app.prompt("GitHub", "Seu usuário do GitHub (o que vem depois de github.com/).",
                        initial=self.cfg.get("usuario", ""), anchor=self.card_rect(), placeholder="usuario",
                        on_done=lambda text, _place: self.set_user(text.strip().lstrip("@")))

    def load_sample(self):
        cached = self.app.cache.get("github")
        self.data = github_from_cache(cached) if cached else None
        if not self.data:
            today = date.today()
            days = [(today - timedelta(days=i), (i * 7 + i // 3) % 5 if i % 4 else 0, i % 6)
                    for i in range(370, -1, -1)]
            now = datetime.now(timezone.utc)
            rows = [{"icon": icon, "title": title, "info": info, "when": now - timedelta(hours=h), "url": ""}
                    for icon, title, info, h in (
                        ("commit", "Ajusta o layout do cartão", "meu-projeto", 1),
                        ("pr_merged", "Adiciona modo escuro", "meu-projeto  ·  #42", 5),
                        ("star", "Deu estrela", "python/cpython", 26))]
            self.data = {"user": "voce", "days": days, "total": 812, "rows": rows}
        self.msg, self.scroll = "", 0

    def set_user(self, user):
        self.cfg["usuario"] = user
        self.save()
        self.data, self.msg = None, "Atualizando…"
        self.app.cache.pop("reviews_seen", None)
        self.refresh()
        self.update_data()

    def retry(self):
        self.update_data()

    def menu_items(self):
        return [("cmd", "Atualizar", self.update_data),
                ("cmd", "Abrir perfil no GitHub", self._open_profile),
                ("cmd", "Mudar usuário…", self.ask_user)]

    def _open_profile(self):
        user = (self.data or {}).get("user") or self.cfg.get("usuario")
        webbrowser.open(f"https://github.com/{user}" if user else "https://github.com")

    def _streak(self):
        counts = {d: c for d, _, c in self.data["days"]}
        day = date.today()
        if not counts.get(day):
            day -= timedelta(days=1)  # o dia de hoje ainda pode ganhar contribuições
        streak = 0
        while counts.get(day, 0) > 0:
            streak += 1
            day -= timedelta(days=1)
        return streak, counts.get(date.today(), 0)

    # --- cliques, destaques e dicas ---

    def _visible_rows(self, mode=None):
        geom = self._list_geom(mode)
        if not geom or not self.data:
            return []
        return self.data["rows"][self.scroll:self.scroll + geom[1]]

    def _row_index(self, x, y):
        geom = self._list_geom()
        if not geom or not self.data:
            return None
        top = geom[0]
        i = int((y - top) // self.ROW_H)
        if y >= top and 0 <= i < len(self._visible_rows()) and self.PAD - 6 <= x <= self.W - self.PAD + 6:
            return i
        return None

    def _row_at(self, x, y):
        i = self._row_index(x, y)
        return None if i is None else self._visible_rows()[i]

    def _on_header(self, x, y):
        return self.mode == "g" and 10 <= y <= 30 and self.PAD - 4 <= x <= self.W - self.PAD + 4

    def _on_message(self, x, y):
        """Área do texto de "sem dados" (carregando, erro)."""
        return not self.data and self.error and 8 <= x <= self.W - 8

    def _day_at(self, x, y):
        """(data, quantidade) do quadradinho do mapa em (x, y)."""
        if self.mode == "g" or not self.data:
            return None
        col, row = int((x - self.GRID_X) // self.STEP), int((y - self.GRID_Y) // self.STEP)
        inside = (x - self.GRID_X - col * self.STEP <= self.CELL
                  and y - self.GRID_Y - row * self.STEP <= self.CELL)
        if not (0 <= col < self.WEEKS and 0 <= row < 7 and inside):
            return None
        day = self._grid_start() + timedelta(days=col * 7 + row)
        counts = {d: c for d, _, c in self.data["days"]}
        return (day, counts[day]) if day in counts else None

    def hit(self, x, y):
        shortcuts = self.app.setting("atalhos_clique")
        if self._on_message(x, y):
            return self.update_data
        row = self._row_at(x, y)
        if row and shortcuts:
            return lambda: webbrowser.open(row["url"])
        if self._on_header(x, y) and shortcuts:
            return self._open_profile
        return None

    def hot_rect(self, x, y):
        i = self._row_index(x, y)
        if i is not None:
            top = self._list_geom()[0] + i * self.ROW_H
            return self.PAD - 8, top - 4, self.W - self.PAD + 6, top + self.ROW_H - 6, 10
        if self._on_header(x, y):
            return self.PAD - 6, 10, self.W - self.PAD + 6, 30, 8
        if self._on_message(x, y):
            return 8, 8, self.W - 8, self.layout(self.mode)["h"] - 8, 14
        return None

    def tooltip(self, x, y):
        cell = self._day_at(x, y)
        if cell:
            day, count = cell
            amount = {0: "Nenhuma contribuição", 1: "1 contribuição"}.get(count, f"{count} contribuições")
            return f"{amount}\n{WEEKDAYS[day.weekday()]}, {day.day} de {MONTHS[day.month - 1]}"
        row = self._row_at(x, y)
        if row:
            when = row["when"].astimezone()
            title = "\n".join(textwrap.wrap(row["title"], 48)[:4])
            text = f"{title}\n\n{when.day} de {MONTHS[when.month - 1]} às {when:%H:%M}"
            return text + ("\nClique para abrir no GitHub" if self.app.setting("atalhos_clique") else "")
        if self._on_header(x, y):
            lines = ["Abrir o perfil no GitHub"]
            if self.updated:
                lines.append("Atualizado " + time_ago(datetime.fromtimestamp(self.updated, timezone.utc)))
            return "\n".join(lines)
        return None

    def on_wheel(self, x, y, steps):
        geom = self._list_geom()
        if not geom or not self.data:
            return
        top = max(0, len(self.data["rows"]) - geom[1])
        scroll = min(max(self.scroll - round(steps), 0), top)
        if scroll != self.scroll:
            self.scroll = scroll
            self.refresh()

    # --- desenho ---

    def draw_shapes(self, d, k, mode):
        if not self.data:
            return
        if mode != "g":
            self._heatmap(d, k)
        geom = self._list_geom(mode)
        if not geom:
            return
        list_y, visible = geom
        rows = self.data["rows"]
        if len(rows) > visible:  # barra de rolagem fininha
            x, y0 = (self.W - 7) * k, list_y * k
            track = (visible * self.ROW_H - 10) * k
            w = 3 * k
            d.rounded_rectangle((x, y0, x + w, y0 + track), radius=w / 2, fill=translucent((255, 255, 255), 0.08))
            thumb = track * visible / len(rows)
            scroll = min(self.scroll, len(rows) - visible)
            ty = y0 + (track - thumb) * scroll / (len(rows) - visible)
            d.rounded_rectangle((x, ty, x + w, ty + thumb), radius=w / 2, fill=translucent((255, 255, 255), 0.4))
        for i, row in enumerate(self._visible_rows(mode)):
            color = KIND_COLOR[row["icon"]]
            cx, cy = (self.PAD + self.ICON_D / 2) * k, (list_y + i * self.ROW_H + 17) * k
            r = self.ICON_D / 2 * k
            bg = translucent(color, 0.18)
            d.ellipse((cx - r, cy - r, cx + r, cy + r), fill=bg)
            draw_glyph(d, row["icon"], cx, cy, 13 * k, color + (255,), bg)

    def _grid_start(self):
        """Primeiro dia do mapa: colunas são semanas de domingo a sábado; a última é a atual."""
        last = self.data["days"][-1][0]
        return last - timedelta(days=(last.weekday() + 1) % 7 + 7 * (self.WEEKS - 1))

    def _heatmap(self, d, k):
        start = self._grid_start()
        today = date.today()
        for day, level, _count in self.data["days"]:
            offset = (day - start).days
            if offset < 0:
                continue
            x = (self.GRID_X + offset // 7 * self.STEP) * k
            y = (self.GRID_Y + offset % 7 * self.STEP) * k
            box = (x, y, x + self.CELL * k, y + self.CELL * k)
            d.rounded_rectangle(box, radius=self.CELL * 0.25 * k, fill=HEAT[min(level, 4)])
            if day == today:
                d.rounded_rectangle(box, radius=self.CELL * 0.25 * k, outline=(255, 255, 255, 220),
                                    width=max(1, round(1.2 * k)))

    def _rows_text(self, d, mode):
        list_y, _ = self._list_geom(mode)
        if not self.data["rows"]:
            self.text(d, (self.PAD, list_y + 6), "Nenhuma atividade recente", "segoeui.ttf", 12, MUTED)
        x = self.PAD + self.ICON_D + 10
        w = self.W - x - self.PAD
        for i, row in enumerate(self._visible_rows(mode)):
            top = list_y + i * self.ROW_H
            self.text(d, (x, top + 1), row["title"], "seguisb.ttf", 12, max_w=w)
            info = "  ·  ".join(part for part in (row["info"], time_ago(row["when"])) if part)
            self.text(d, (x, top + 19), info, "segoeui.ttf", 10.5, MUTED, max_w=w)

    def draw_content(self, img, d, mode):
        W, PAD = self.W, self.PAD
        data = self.data
        if mode != "g":
            if not data:
                h = self.layout(mode)["h"]
                self.text(d, (W / 2, h / 2), self.msg, "seguisb.ttf", 12, anchor="mm", max_w=W - 2 * self.C_PAD)
                return
            if mode == "m":
                y = round((self.M_LIST_Y - 6) * self.s)
                d.line((PAD * self.s, y, (W - PAD) * self.s, y), fill=DIVIDER, width=max(1, round(self.s)))
                self._rows_text(d, mode)
            return

        self.text(d, (PAD, 19), "GITHUB", "seguisb.ttf", 10, GITHUB_GREEN)
        user = (data or {}).get("user") or self.cfg.get("usuario")
        if user:
            self.text(d, (W - PAD, 19), f"@{user}", "segoeui.ttf", 10.5, MUTED, anchor="ra", max_w=180)
        if not data:
            self.text(d, (PAD, 36), self.msg, "seguisb.ttf", 14, max_w=W - 2 * PAD)
            return
        self.text(d, (PAD, 35), f"{data['total']:,}".replace(",", ".") + " contribuições no último ano",
                  "seguisb.ttf", 14, max_w=W - 2 * PAD)
        streak, today = self._streak()
        days = "dia" if streak == 1 else "dias"
        self.text(d, (PAD, 56), f"Sequência de {streak} {days}  ·  Hoje: {today}", "segoeui.ttf", 11, MUTED)

        y = round((self.LIST_Y - 10) * self.s)
        d.line((PAD * self.s, y, (W - PAD) * self.s, y), fill=DIVIDER, width=max(1, round(self.s)))
        self._rows_text(d, mode)
