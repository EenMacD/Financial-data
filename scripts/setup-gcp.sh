#!/usr/bin/env bash
# One-time GCP bootstrap for the GitHub Actions -> Cloud Run deploy.
#
# Prereq: `gcloud auth login` with an account that owns/admins the project.
# Usage:  bash scripts/setup-gcp.sh <PROJECT_ID>
#
# Creates a `gh-deployer` service account with the roles Cloud Build + Cloud Run
# need, and writes its key to ./gh-sa-key.json. Add that file's contents to the
# GitHub secret GCP_SA_KEY, then delete it.
set -euo pipefail

PROJECT_ID="${1:-}"
if [ -z "$PROJECT_ID" ]; then
  echo "usage: bash scripts/setup-gcp.sh <PROJECT_ID>" >&2
  exit 1
fi

SA_NAME="gh-deployer"
SA_EMAIL="${SA_NAME}@${PROJECT_ID}.iam.gserviceaccount.com"
KEY_FILE="gh-sa-key.json"

echo "▸ Project: $PROJECT_ID"
gcloud config set project "$PROJECT_ID" >/dev/null

echo "▸ Enabling APIs (run, cloudbuild, artifactregistry)…"
gcloud services enable \
  run.googleapis.com \
  cloudbuild.googleapis.com \
  artifactregistry.googleapis.com

if gcloud iam service-accounts describe "$SA_EMAIL" >/dev/null 2>&1; then
  echo "▸ Service account $SA_EMAIL already exists"
else
  echo "▸ Creating service account $SA_NAME…"
  gcloud iam service-accounts create "$SA_NAME" \
    --display-name "GitHub Actions deployer"
fi

echo "▸ Granting deploy roles…"
for ROLE in \
  roles/run.admin \
  roles/cloudbuild.builds.editor \
  roles/artifactregistry.admin \
  roles/storage.admin \
  roles/iam.serviceAccountUser; do
  gcloud projects add-iam-policy-binding "$PROJECT_ID" \
    --member "serviceAccount:${SA_EMAIL}" \
    --role "$ROLE" \
    --condition=None \
    --quiet >/dev/null
  echo "    granted $ROLE"
done

echo "▸ Creating key -> $KEY_FILE…"
gcloud iam service-accounts keys create "$KEY_FILE" --iam-account "$SA_EMAIL"

cat <<EOF

✓ Done. Next:
    gh secret set GCP_SA_KEY < $KEY_FILE
    gh secret set GCP_PROJECT_ID -b "$PROJECT_ID"
  then remove the local key:
    rm $KEY_FILE
EOF
