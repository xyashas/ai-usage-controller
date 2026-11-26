# Build from the repo root:  docker build -f api/Dockerfile -t aiuc-api .
FROM node:22-slim AS build
WORKDIR /app/api
COPY api/package.json api/package-lock.json ./
RUN npm ci
COPY api/tsconfig.json ./
COPY api/src ./src
RUN npm run build && npm prune --omit=dev

FROM node:22-slim
ENV NODE_ENV=production PORT=8787 DATABASE_PATH=/data/aiuc.db TABLES_DIR=/app/core/data
WORKDIR /app/api
COPY --from=build /app/api/node_modules ./node_modules
COPY --from=build /app/api/dist ./dist
COPY api/package.json ./
COPY core/data /app/core/data
VOLUME /data
EXPOSE 8787
CMD ["node", "--disable-warning=ExperimentalWarning", "dist/server.js"]
