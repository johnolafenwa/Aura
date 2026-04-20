"use strict";

function createDocumentStateCache(analyzeDocument) {
  const documentStates = new Map();

  async function get(document) {
    const existing = documentStates.get(document.uri);
    if (existing && existing.version === document.version) {
      return existing;
    }

    const compilerAnalysis = await analyzeDocument(document);
    const next = {
      version: document.version,
      compilerAnalysis
    };
    documentStates.set(document.uri, next);
    return next;
  }

  function invalidateAll() {
    documentStates.clear();
  }

  function deleteDocument(uri) {
    documentStates.delete(uri);
  }

  return {
    deleteDocument,
    get,
    invalidateAll
  };
}

async function validateOpenDocuments(documents, validateDocument) {
  for (const document of documents.all()) {
    await validateDocument(document);
  }
}

module.exports = {
  createDocumentStateCache,
  validateOpenDocuments
};
