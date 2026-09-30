FROM node:18-alpine

WORKDIR /app

# Copy package files
COPY examples/events-webhook-relay/package*.json ./

# Install dependencies
RUN npm ci --only=production && \
    npm run build

# Copy source (already compiled at this point)
COPY examples/events-webhook-relay/dist ./dist

# Health check
HEALTHCHECK --interval=30s --timeout=10s --start-period=5s --retries=3 \
  CMD node -e "require('http').get('http://localhost:' + (process.env.PORT || 3000) + '/health', (r) => {if (r.statusCode !== 200) throw new Error(r.statusCode)})"

# Expose port
EXPOSE 3000

# Run
CMD ["npm", "start"]
