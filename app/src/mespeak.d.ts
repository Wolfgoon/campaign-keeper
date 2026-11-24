declare module "mespeak" {
  const meSpeak: { loadConfig(c: any): void; loadVoice(v: any): void; speak(text: string, opts?: Record<string, unknown>): any };
  export default meSpeak;
}
