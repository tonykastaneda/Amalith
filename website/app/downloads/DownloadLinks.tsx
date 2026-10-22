export type Platform = {
  name: string;
  icon: string;
};

export function DownloadLinks({ platforms }: { platforms: Platform[] }) {
  return (
    <div className="download-actions" aria-label="Desktop availability">
      {platforms.map((platform) => (
        <div className="platform-download platform-download--unavailable" key={platform.name}>
          <img src={platform.icon} alt="" aria-hidden="true" />
          <span>{platform.name} — Coming soon</span>
        </div>
      ))}
    </div>
  );
}
