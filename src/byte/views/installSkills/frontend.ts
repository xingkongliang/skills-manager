import { invoke } from "@tauri-apps/api/core";

export type Board = "all" | "biweekly" | "monthly";
export interface MarketSkill {
  id: string;
  identifier: string;
  repository: string;
  name: string;
  description: string;
  version?: string | null;
  downloads: number;
  download_total: number;
  download_biweekly: number;
  download_monthly: number;
  disabled: boolean;
  no_permission: boolean;
  url: string;
}
export interface MarketPage {
  skills: MarketSkill[];
  total: number;
  page: number;
  page_size: number;
}
export interface BrowseRequest {
  board: Board;
  query: string;
  page: number;
  pageSize: number;
  refresh: boolean;
}
export const fetchSkills = (request: BrowseRequest) =>
  invoke<MarketPage>("fetch_bytedance_skills", { ...request });
export const installSkill = (skill: MarketSkill) =>
  invoke<string>("install_from_bytedance", {
    identifier: skill.identifier,
    repository: skill.repository,
  });
export const cancelKey = (skill: MarketSkill) =>
  `bytedance:${skill.repository}:${skill.identifier}`;
export const cancelInstall = (key: string) => invoke<boolean>("cancel_bytedance_install", { key });
