# The static chat UI. No runtime, just files.
FROM node:22-slim AS build
WORKDIR /src
COPY package.json package-lock.json* ./
RUN npm install
COPY . .
RUN npm run build

FROM nginx:alpine
# nginx:alpine runs envsubst over everything in this directory at startup, so
# the token reaches the config without being baked into the image.
COPY nginx.conf.template /etc/nginx/templates/default.conf.template
COPY --from=build /src/index.html /usr/share/nginx/html/index.html
COPY --from=build /src/styles.css /usr/share/nginx/html/styles.css
COPY --from=build /src/dist /usr/share/nginx/html/dist
COPY --from=build /src/fonts /usr/share/nginx/html/fonts
EXPOSE 80
