"""Utilidades sem interface: rede, datas em português e tempo relativo."""

import json
import urllib.parse
import urllib.request
from datetime import date, datetime, timezone

MONTHS = ("jan", "fev", "mar", "abr", "mai", "jun", "jul", "ago", "set", "out", "nov", "dez")
WEEKDAYS = ("seg", "ter", "qua", "qui", "sex", "sáb", "dom")
MONTHS_LONG = ("janeiro", "fevereiro", "março", "abril", "maio", "junho", "julho", "agosto", "setembro",
               "outubro", "novembro", "dezembro")
WEEKDAYS_LONG = ("segunda-feira", "terça-feira", "quarta-feira", "quinta-feira", "sexta-feira", "sábado",
                 "domingo")


def fetch_json(url):
    req = urllib.request.Request(url, headers={"User-Agent": "desktop-widget"})
    with urllib.request.urlopen(req, timeout=15) as resp:
        return json.load(resp)


def long_date(day=None):
    """"sábado, 31 de outubro"."""
    day = day or date.today()
    return f"{WEEKDAYS_LONG[day.weekday()]}, {day.day} de {MONTHS_LONG[day.month - 1]}"


def time_ago(when):
    seconds = (datetime.now(timezone.utc) - when).total_seconds()
    if seconds < 60:
        return "agora"
    if seconds < 3600:
        return f"há {int(seconds // 60)} min"
    if seconds < 86400:
        return f"há {int(seconds // 3600)} h"
    local = when.astimezone()
    days = (date.today() - local.date()).days
    if days == 1:
        return "ontem"
    if days < 7:
        return f"há {days} dias"
    return f"{local.day} {MONTHS[local.month - 1]}"
