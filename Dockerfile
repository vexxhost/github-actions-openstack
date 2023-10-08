FROM python:3.10-slim AS builder
WORKDIR /app/
RUN \
  apt-get update && \
  apt-get install -y --no-install-recommends gcc libc6-dev
RUN \
  --mount=type=cache,target=/root/.cache/pip \
  pip install poetry
RUN poetry config virtualenvs.in-project true
ADD pyproject.toml poetry.lock /app/
RUN \
  --mount=type=cache,target=/root/.cache/pypoetry \
  poetry install --no-root --no-interaction --no-ansi

FROM python:3.10-slim
ENV PATH="/app/.venv/bin:$PATH"
COPY --from=builder /app/.venv /app/.venv
COPY . /app/

WORKDIR /app/
CMD ["uwsgi", "--ini", "contrib/uwsgi.ini"]
