FROM debian:sid-slim
RUN apt -y update && apt install -y git dctrl-tools
COPY . /code
RUN apt satisfy -y "$(grep-dctrl -n -w -s Build-Depends '' /code/debian/control)"
ENV PYTHONPATH=/code
ENTRYPOINT ["/code/scripts/lintian-brush", "-d", "/data"]
