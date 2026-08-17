# The static chat UI. No runtime, just files.
FROM node:22-slim AS build
WORKDIR /src
COPY package.json package-lock.json* ./
RUN npm install
COPY . .
RUN npm run build

FROM nginx:alpine
COPY --from=build /src/index.html /usr/share/nginx/html/index.html
COPY --from=build /src/styles.css /usr/share/nginx/html/styles.css
COPY --from=build /src/dist /usr/share/nginx/html/dist
EXPOSE 80
