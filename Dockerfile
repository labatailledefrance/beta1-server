# syntax=docker/dockerfile:1
# Serveur : construit sur l'image Rust durcie de Docker (dhi.io), exécuté sur
# l'image « static » avec glibc : ni shell, ni gestionnaire de paquets, non root.
FROM dhi.io/rust:1.100.0-debian13-dev@sha256:70b12263cf778cba39a48563b13b55d08ef4c7db5df648d1e1a87c398e1f5f48 AS construction
WORKDIR /src
COPY . .
RUN --mount=type=cache,target=/root/.cargo/registry \
    --mount=type=cache,target=/root/.cargo/git \
    cargo build --release --locked \
 && cp target/release/beta1-server /beta1-server

FROM dhi.io/static:20250419-glibc-debian13@sha256:133e7708dab3dc8c84080758eb1b99ef63795ff39a8588d038db4d3c2abacfba
COPY --from=construction /beta1-server /beta1-server
# Utilisateur « nonroot » de l'image de base, explicite.
USER 65532:65532
EXPOSE 7425
ENTRYPOINT ["/beta1-server"]
