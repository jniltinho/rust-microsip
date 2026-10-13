# syntax=docker/dockerfile:1.19.0@sha256:b6afd42430b15f2d2a4c5a02b919e98a525b785b1aaff16747d2f623364e39b6
# See docs/adr/0002-disposable-asterisk-lab.md for provenance and license limits.
FROM andrius/asterisk:22.11.0_debian-trixie@sha256:eaa6aaaa09ede2829514769d5d1aba3041ec8b3055899feebfc8e8a9a1af7c0f AS upstream

# Discard inherited anonymous VOLUME, root entrypoint and generic healthcheck.
# This separate test image is never embedded in the application or its artifacts.
FROM scratch AS runtime
COPY --from=upstream / /
ENV PATH="/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin" \
    PYTHONDONTWRITEBYTECODE=1
COPY lab.py sip_probe.py verify_compose.py /opt/lab/
COPY tests/ /opt/lab/tests/
RUN asterisk -V | grep -Fx 'Asterisk 22.11.0' \
    && python3 -c 'import sys; from pathlib import Path; sys.path.insert(0, "/opt/lab"); import lab; assert all((Path("/usr/lib/asterisk/modules") / (name + ".so")).is_file() for name in lab.MODULES)' \
    && mkdir -p /run/asterisk /lab && chown 1000:1000 /run/asterisk
USER 1000:1000
WORKDIR /opt/lab
ENTRYPOINT ["python3", "/opt/lab/lab.py"]
CMD ["pbx"]
