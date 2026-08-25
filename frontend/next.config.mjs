/**
 * `standalone` because the deployment is a container: the output carries the
 * server and only the dependencies it actually reached, so the runtime image
 * does not ship `node_modules` entire.
 *
 * @type {import('next').NextConfig}
 */
const config = {
  output: "standalone",
  poweredByHeader: false,
  reactStrictMode: true,
};

export default config;
