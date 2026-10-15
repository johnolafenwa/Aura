"use strict";

// `dependenciesForAnalysis(analysis, previous)` returns the URIs a document's
// analysis read. `previous` is the set the last analysis found.
function createDocumentStateCache(analyzeDocument, dependenciesForAnalysis = () => []) {
  const documentStates = new Map();
  const generations = new Map();
  const dependencies = new Map();
  const reverseDependencies = new Map();

  function generation(uri) {
    return generations.get(uri) || 0;
  }

  function updateDependencies(uri, nextDependencies) {
    for (const dependency of dependencies.get(uri) || []) {
      const dependents = reverseDependencies.get(dependency);
      if (dependents) {
        dependents.delete(uri);
        if (dependents.size === 0) {
          reverseDependencies.delete(dependency);
        }
      }
    }

    const next = new Set([...nextDependencies].filter((dependency) => dependency !== uri));
    dependencies.set(uri, next);
    for (const dependency of next) {
      if (!reverseDependencies.has(dependency)) {
        reverseDependencies.set(dependency, new Set());
      }
      reverseDependencies.get(dependency).add(uri);
    }
  }

  async function get(document) {
    const existing = documentStates.get(document.uri);
    if (existing && existing.version === document.version) {
      return existing.state || existing.promise;
    }

    const startedGeneration = generation(document.uri);
    const analysis = analyzeDocument(document);
    let promise;
    promise = Promise.resolve(analysis)
      .then((compilerAnalysis) => {
        const state = {
          version: document.version,
          compilerAnalysis
        };
        const current = documentStates.get(document.uri);
        if (
          current &&
          current.promise === promise &&
          generation(document.uri) === startedGeneration
        ) {
          documentStates.set(document.uri, {
            version: document.version,
            state
          });
          updateDependencies(
            document.uri,
            dependenciesForAnalysis(compilerAnalysis, dependencies.get(document.uri) || new Set())
          );
        }
        return state;
      })
      .catch((error) => {
        const current = documentStates.get(document.uri);
        if (current && current.promise === promise) {
          documentStates.delete(document.uri);
        }
        throw error;
      });
    documentStates.set(document.uri, {
      version: document.version,
      promise
    });
    return promise;
  }

  function invalidate(uri) {
    const affected = new Set();
    const pending = [uri];
    while (pending.length > 0) {
      const current = pending.pop();
      if (affected.has(current)) {
        continue;
      }
      affected.add(current);
      for (const dependent of reverseDependencies.get(current) || []) {
        pending.push(dependent);
      }
    }
    for (const affectedUri of affected) {
      generations.set(affectedUri, generation(affectedUri) + 1);
      documentStates.delete(affectedUri);
    }
    return affected;
  }

  // Invalidates every document whose state matches, with its dependents.
  // An analysis still in flight is passed as `null`.
  function invalidateWhere(predicate) {
    const affected = new Set();
    const matching = [...documentStates]
      .filter(([, entry]) => predicate(entry.state || null))
      .map(([uri]) => uri);
    for (const uri of matching) {
      for (const affectedUri of invalidate(uri)) {
        affected.add(affectedUri);
      }
    }
    return affected;
  }

  function invalidateAll() {
    const affected = new Set(documentStates.keys());
    for (const uri of affected) {
      generations.set(uri, generation(uri) + 1);
    }
    documentStates.clear();
    return affected;
  }

  function deleteDocument(uri) {
    generations.set(uri, generation(uri) + 1);
    documentStates.delete(uri);
    updateDependencies(uri, []);
    dependencies.delete(uri);
  }

  return {
    deleteDocument,
    get,
    invalidate,
    invalidateAll,
    invalidateWhere
  };
}

async function validateOpenDocuments(documents, validateDocument) {
  await Promise.all(documents.all().map((document) => validateDocument(document)));
}

module.exports = {
  createDocumentStateCache,
  validateOpenDocuments
};
