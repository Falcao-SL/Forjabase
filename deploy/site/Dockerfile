# Site do Forja — https://forja.falcaosl.com
#
# HTML e CSS sem etapa de build: a imagem é um COPY para dentro do nginx e nada
# mais. O contexto de build é a raiz do repositório, como nos outros apps.
#
#   docker build -f deploy/site/Dockerfile -t forja-site .
#   docker run --rm -p 8080:80 forja-site

FROM nginx:alpine

COPY deploy/site/nginx.conf /etc/nginx/conf.d/default.conf
COPY deploy/site/security-headers.conf /etc/nginx/snippets/forja-security-headers.conf

COPY site/ /usr/share/nginx/html/
RUN nginx -t

EXPOSE 80
