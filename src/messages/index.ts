// Every language's messages, by locale tag (lib/i18n reads them; English is the source of truth).
import { messages as de } from "@/messages/de";
import { messages as en } from "@/messages/en";
import { messages as es } from "@/messages/es";
import { messages as fr } from "@/messages/fr";
import { messages as ja } from "@/messages/ja";
import { messages as ptBR } from "@/messages/pt-BR";

export const CATALOGS = { en, fr, es, de, "pt-BR": ptBR, ja };
