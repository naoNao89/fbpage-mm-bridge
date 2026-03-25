-- Create customers table for Customer Service
-- This is the core table for managing customer identity and profiles

CREATE TABLE IF NOT EXISTS customers (
    id UUID PRIMARY KEY,
    platform_user_id VARCHAR(255) NOT NULL,
    platform VARCHAR(50) NOT NULL,
    name VARCHAR(255),
    phone VARCHAR(50),
    created_at TIMESTAMP WITH TIME ZONE NOT NULL DEFAULT NOW(),
    
    -- Ensure unique customer per platform
    CONSTRAINT unique_platform_customer UNIQUE (platform_user_id, platform)
);

-- Index for efficient lookups by platform user ID
CREATE INDEX IF NOT EXISTS idx_customers_platform_user_id ON customers(platform_user_id);
CREATE INDEX IF NOT EXISTS idx_customers_platform ON customers(platform);
CREATE INDEX IF NOT EXISTS idx_customers_created_at ON customers(created_at DESC);

-- Comment on table
COMMENT ON TABLE customers IS 'Customer profiles from various platforms (Facebook, Zalo, etc.)';
COMMENT ON COLUMN customers.id IS 'Unique identifier for the customer';
COMMENT ON COLUMN customers.platform_user_id IS 'User ID from the platform (e.g., Facebook PSID)';
COMMENT ON COLUMN customers.platform IS 'Platform identifier (facebook, zalo)';
COMMENT ON COLUMN customers.name IS 'Customer display name';
COMMENT ON COLUMN customers.phone IS 'Customer phone number (if available)';
COMMENT ON COLUMN customers.created_at IS 'Timestamp when customer was first created';