import { createContext } from "react";
import type { DefinitionLibrary } from "./definitionLibrary";
export const DefinitionWorkspace = createContext<{
  library: DefinitionLibrary;
  update: (library: DefinitionLibrary) => void;
  storageError: string;
  projectLocal: boolean;
} | null>(null);
