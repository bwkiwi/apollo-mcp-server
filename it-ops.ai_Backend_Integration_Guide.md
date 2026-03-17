# Apollo MCP Server Phase 2 - Backend GraphQL Integration Guide

## Overview

This guide explains how to integrate your GraphQL server backend with the Apollo MCP Server Phase 2 Auth0 authentication system. Your GraphQL server will receive JWT tokens and needs to validate them against Auth0.

## Current Configuration

Based on your `config-phase2.yaml`:

```yaml
# GraphQL endpoint that will receive authenticated requests
endpoint: http://host.docker.internal:8088/graphql

# Auth0 configuration
auth0:
  domain: monovandi.eu.auth0.com
  client_id: 9iFHpiJqhQl6KCel7Qe1OlbvWllz2xJj
  audience: https://api.it-ops.ai
```

## JWT Token Flow

```
┌─────────────────┐    1. GraphQL Request    ┌──────────────────────┐
│  Apollo MCP     │ ──────────────────────▶ │   Your GraphQL      │
│  Server         │                         │   Server             │
│                 │    Authorization:       │   (Port 8088)        │
│  (Port 5000)    │    Bearer <JWT_TOKEN>   │                      │
└─────────────────┘                         └──────────────────────┘
         ▲                                             │
         │                                             │
         │ 2. JWT from Auth0                           │ 3. Validate JWT
         │                                             ▼
┌─────────────────┐                         ┌──────────────────────┐
│     Auth0       │                         │      Auth0           │
│                 │◀────────────────────────│   Validation         │
│ monovandi.eu    │   4. JWT Validation     │   (JWKS endpoint)    │
│ .auth0.com      │                         │                      │
└─────────────────┘                         └──────────────────────┘
```

## JWT Token Structure

The JWT tokens your GraphQL server receives will have this structure:

### Header
```json
{
  "alg": "RS256",
  "typ": "JWT",
  "kid": "key-id-from-auth0"
}
```

### Payload
```json
{
  "iss": "https://monovandi.eu.auth0.com/",
  "sub": "auth0|user-id",
  "aud": ["https://api.it-ops.ai"],
  "iat": 1703764800,
  "exp": 1703768400,
  "azp": "9iFHpiJqhQl6KCel7Qe1OlbvWllz2xJj",
  "scope": "openid profile email",
  "permissions": ["read:data", "write:data"],
  "https://api.it-ops.ai/user_metadata": {
    "groups": ["admin", "users"],
    "roles": ["administrator"]
  }
}
```

### Key Fields for Your Backend

- **`iss`**: Must be `https://monovandi.eu.auth0.com/`
- **`aud`**: Must include `https://api.it-ops.ai`
- **`sub`**: User identifier for your application
- **`exp`**: Token expiration (Unix timestamp)
- **`permissions`**: Array of permissions granted to user
- **Custom claims**: User metadata like groups and roles

## Backend Implementation

### 1. JWT Validation Middleware

#### Node.js/Express Example

```javascript
const jwt = require('jsonwebtoken');
const jwksClient = require('jwks-rsa');

// JWKS client configuration for Auth0
const client = jwksClient({
  jwksUri: 'https://monovandi.eu.auth0.com/.well-known/jwks.json',
  requestHeaders: {}, 
  timeout: 30000,
});

function getKey(header, callback) {
  client.getSigningKey(header.kid, (err, key) => {
    const signingKey = key.publicKey || key.rsaPublicKey;
    callback(null, signingKey);
  });
}

// JWT validation middleware
function validateJWT(req, res, next) {
  const authHeader = req.headers.authorization;
  
  if (!authHeader || !authHeader.startsWith('Bearer ')) {
    return res.status(401).json({ 
      error: 'Missing or invalid Authorization header' 
    });
  }

  const token = authHeader.substring(7); // Remove 'Bearer ' prefix

  jwt.verify(token, getKey, {
    audience: 'https://api.it-ops.ai',
    issuer: 'https://monovandi.eu.auth0.com/',
    algorithms: ['RS256']
  }, (err, decoded) => {
    if (err) {
      console.error('JWT validation failed:', err.message);
      return res.status(401).json({ 
        error: 'Invalid token' 
      });
    }

    // Add user info to request context
    req.user = {
      id: decoded.sub,
      permissions: decoded.permissions || [],
      groups: decoded['https://api.it-ops.ai/user_metadata']?.groups || [],
      roles: decoded['https://api.it-ops.ai/user_metadata']?.roles || []
    };

    next();
  });
}

// Apply to GraphQL endpoint
app.use('/graphql', validateJWT, graphqlHandler);
```

#### Python/FastAPI Example

```python
import jwt
import requests
from fastapi import HTTPException, Depends
from fastapi.security import HTTPBearer, HTTPAuthorizationCredentials

# Auth0 configuration
AUTH0_DOMAIN = "monovandi.eu.auth0.com"
AUTH0_AUDIENCE = "https://api.it-ops.ai"
JWKS_URL = f"https://{AUTH0_DOMAIN}/.well-known/jwks.json"

security = HTTPBearer()

class Auth0User:
    def __init__(self, user_id: str, permissions: list, groups: list, roles: list):
        self.id = user_id
        self.permissions = permissions
        self.groups = groups
        self.roles = roles

def get_jwks():
    """Fetch JWKS from Auth0"""
    response = requests.get(JWKS_URL)
    return response.json()

def validate_token(credentials: HTTPAuthorizationCredentials = Depends(security)) -> Auth0User:
    """Validate JWT token and return user info"""
    try:
        # Get JWKS
        jwks = get_jwks()
        
        # Decode token header to get key ID
        unverified_header = jwt.get_unverified_header(credentials.credentials)
        
        # Find the correct key
        rsa_key = {}
        for key in jwks["keys"]:
            if key["kid"] == unverified_header["kid"]:
                rsa_key = {
                    "kty": key["kty"],
                    "kid": key["kid"],
                    "use": key["use"],
                    "n": key["n"],
                    "e": key["e"]
                }
                break
        
        if not rsa_key:
            raise HTTPException(status_code=401, detail="Unable to find appropriate key")
        
        # Validate token
        payload = jwt.decode(
            credentials.credentials,
            rsa_key,
            algorithms=["RS256"],
            audience=AUTH0_AUDIENCE,
            issuer=f"https://{AUTH0_DOMAIN}/"
        )
        
        # Extract user information
        user_metadata = payload.get(f'{AUTH0_AUDIENCE}/user_metadata', {})
        
        return Auth0User(
            user_id=payload['sub'],
            permissions=payload.get('permissions', []),
            groups=user_metadata.get('groups', []),
            roles=user_metadata.get('roles', [])
        )
        
    except jwt.ExpiredSignatureError:
        raise HTTPException(status_code=401, detail="Token has expired")
    except jwt.InvalidTokenError as e:
        raise HTTPException(status_code=401, detail="Invalid token")

# Use in GraphQL context
@app.post("/graphql")
async def graphql_endpoint(user: Auth0User = Depends(validate_token)):
    # User is now available in GraphQL resolvers
    context = {"user": user}
    return await graphql_app.handle_request(request, context=context)
```

#### Go Example

```go
package main

import (
    "context"
    "fmt"
    "net/http"
    "strings"
    "time"
    
    "github.com/dgrijalva/jwt-go"
    "github.com/lestrrat-go/jwx/jwk"
)

type Auth0User struct {
    ID          string   `json:"id"`
    Permissions []string `json:"permissions"`
    Groups      []string `json:"groups"`
    Roles       []string `json:"roles"`
}

type contextKey string

const UserContextKey contextKey = "user"

func validateJWT(next http.Handler) http.Handler {
    return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
        authHeader := r.Header.Get("Authorization")
        if authHeader == "" || !strings.HasPrefix(authHeader, "Bearer ") {
            http.Error(w, "Missing or invalid Authorization header", http.StatusUnauthorized)
            return
        }

        tokenString := authHeader[7:] // Remove 'Bearer ' prefix

        // Fetch JWKS
        set, err := jwk.Fetch(context.Background(), "https://monovandi.eu.auth0.com/.well-known/jwks.json")
        if err != nil {
            http.Error(w, "Failed to fetch JWKS", http.StatusInternalServerError)
            return
        }

        // Parse and validate token
        token, err := jwt.Parse(tokenString, func(token *jwt.Token) (interface{}, error) {
            if _, ok := token.Method.(*jwt.SigningMethodRSA); !ok {
                return nil, fmt.Errorf("unexpected signing method: %v", token.Header["alg"])
            }

            kid, ok := token.Header["kid"].(string)
            if !ok {
                return nil, fmt.Errorf("kid header not found")
            }

            key, ok := set.LookupKeyID(kid)
            if !ok {
                return nil, fmt.Errorf("key not found")
            }

            var rawKey interface{}
            if err := key.Raw(&rawKey); err != nil {
                return nil, fmt.Errorf("failed to get raw key")
            }

            return rawKey, nil
        })

        if err != nil || !token.Valid {
            http.Error(w, "Invalid token", http.StatusUnauthorized)
            return
        }

        claims, ok := token.Claims.(jwt.MapClaims)
        if !ok {
            http.Error(w, "Invalid token claims", http.StatusUnauthorized)
            return
        }

        // Validate audience and issuer
        if !claims.VerifyAudience("https://api.it-ops.ai", true) {
            http.Error(w, "Invalid audience", http.StatusUnauthorized)
            return
        }

        if !claims.VerifyIssuer("https://monovandi.eu.auth0.com/", true) {
            http.Error(w, "Invalid issuer", http.StatusUnauthorized)
            return
        }

        // Extract user information
        user := &Auth0User{
            ID: claims["sub"].(string),
        }

        if permissions, ok := claims["permissions"].([]interface{}); ok {
            for _, perm := range permissions {
                user.Permissions = append(user.Permissions, perm.(string))
            }
        }

        // Add user to context
        ctx := context.WithValue(r.Context(), UserContextKey, user)
        next.ServeHTTP(w, r.WithContext(ctx))
    })
}

// Use middleware
http.Handle("/graphql", validateJWT(graphqlHandler))
```

### 2. GraphQL Context Integration

#### Adding User to GraphQL Context

```javascript
// In your GraphQL server setup
const server = new ApolloServer({
  typeDefs,
  resolvers,
  context: ({ req }) => {
    return {
      user: req.user, // Added by JWT middleware
      // ... other context
    };
  },
});
```

#### Using in Resolvers

```javascript
const resolvers = {
  Query: {
    profile: (parent, args, context) => {
      // Check if user is authenticated
      if (!context.user) {
        throw new AuthenticationError('You must be logged in');
      }

      // Check permissions
      if (!context.user.permissions.includes('read:profile')) {
        throw new ForbiddenError('Insufficient permissions');
      }

      // Use user ID for data fetching
      return getUserProfile(context.user.id);
    },

    adminData: (parent, args, context) => {
      // Check for admin role
      if (!context.user?.roles.includes('administrator')) {
        throw new ForbiddenError('Admin access required');
      }

      return getAdminData();
    }
  },

  Mutation: {
    updateProfile: (parent, args, context) => {
      if (!context.user) {
        throw new AuthenticationError('You must be logged in');
      }

      if (!context.user.permissions.includes('write:profile')) {
        throw new ForbiddenError('Insufficient permissions');
      }

      return updateUserProfile(context.user.id, args.input);
    }
  }
};
```

### 3. Permission-Based Access Control

```javascript
// Utility functions for authorization
function requireAuth(user) {
  if (!user) {
    throw new AuthenticationError('Authentication required');
  }
}

function requirePermission(user, permission) {
  requireAuth(user);
  if (!user.permissions.includes(permission)) {
    throw new ForbiddenError(`Permission required: ${permission}`);
  }
}

function requireRole(user, role) {
  requireAuth(user);
  if (!user.roles.includes(role)) {
    throw new ForbiddenError(`Role required: ${role}`);
  }
}

function requireGroup(user, group) {
  requireAuth(user);
  if (!user.groups.includes(group)) {
    throw new ForbiddenError(`Group membership required: ${group}`);
  }
}

// Usage in resolvers
const resolvers = {
  Query: {
    sensitiveData: (parent, args, context) => {
      requirePermission(context.user, 'read:sensitive');
      return getSensitiveData();
    },

    adminPanel: (parent, args, context) => {
      requireRole(context.user, 'administrator');
      return getAdminPanelData();
    }
  }
};
```

## Error Handling

### Standard HTTP Response Codes

```javascript
// Authentication errors
401 Unauthorized: Missing or invalid JWT token
403 Forbidden: Valid token but insufficient permissions

// Example error responses
{
  "errors": [
    {
      "message": "You must be logged in",
      "extensions": {
        "code": "UNAUTHENTICATED"
      }
    }
  ]
}

{
  "errors": [
    {
      "message": "Insufficient permissions",
      "extensions": {
        "code": "FORBIDDEN",
        "required_permission": "read:sensitive"
      }
    }
  ]
}
```

## Testing Your Integration

### 1. Test JWT Validation

```bash
# Get a token from Apollo MCP Server (use getGraphQLToken tool)
TOKEN="Bearer eyJhbGciOiJSUzI1NiIs..."

# Test your GraphQL endpoint
curl -X POST http://localhost:8088/graphql \
  -H "Content-Type: application/json" \
  -H "Authorization: $TOKEN" \
  -d '{"query": "{ __typename }"}'
```

### 2. Test Permission Validation

```bash
# Test with valid permissions
curl -X POST http://localhost:8088/graphql \
  -H "Content-Type: application/json" \
  -H "Authorization: $TOKEN" \
  -d '{"query": "{ profile { id name } }"}'

# Test without required permissions (should return 403)
curl -X POST http://localhost:8088/graphql \
  -H "Content-Type: application/json" \
  -H "Authorization: $TOKEN" \
  -d '{"query": "{ adminData { sensitiveInfo } }"}'
```

### 3. Test Token Expiry

```bash
# Wait for token to expire, then test
# Should return 401 Unauthorized
curl -X POST http://localhost:8088/graphql \
  -H "Content-Type: application/json" \
  -H "Authorization: $EXPIRED_TOKEN" \
  -d '{"query": "{ __typename }"}'
```

## Security Best Practices

### 1. Token Validation

```javascript
// Always validate these JWT claims
const requiredClaims = {
  iss: 'https://monovandi.eu.auth0.com/',
  aud: 'https://api.it-ops.ai',
  exp: 'must be in the future',
  iat: 'must be reasonable',
  sub: 'must be present'
};
```

### 2. JWKS Caching

```javascript
// Cache JWKS to avoid repeated requests
const jwksCache = new Map();
const CACHE_TTL = 3600000; // 1 hour

function getCachedJWKS() {
  const cached = jwksCache.get('jwks');
  if (cached && (Date.now() - cached.timestamp) < CACHE_TTL) {
    return cached.data;
  }
  
  // Fetch new JWKS
  const jwks = fetchJWKS();
  jwksCache.set('jwks', {
    data: jwks,
    timestamp: Date.now()
  });
  
  return jwks;
}
```

### 3. Rate Limiting

```javascript
// Implement rate limiting per user
const rateLimiter = new Map();

function checkRateLimit(userId) {
  const now = Date.now();
  const userLimits = rateLimiter.get(userId) || { count: 0, resetTime: now + 60000 };
  
  if (now > userLimits.resetTime) {
    userLimits.count = 0;
    userLimits.resetTime = now + 60000;
  }
  
  if (userLimits.count >= 100) { // 100 requests per minute
    throw new Error('Rate limit exceeded');
  }
  
  userLimits.count++;
  rateLimiter.set(userId, userLimits);
}
```

### 4. Audit Logging

```javascript
// Log security events
function auditLog(event, user, details = {}) {
  console.log(JSON.stringify({
    timestamp: new Date().toISOString(),
    event,
    userId: user?.id,
    userGroups: user?.groups,
    ...details
  }));
}

// Usage
auditLog('AUTH_SUCCESS', context.user);
auditLog('AUTH_FAILURE', null, { reason: 'Invalid token' });
auditLog('PERMISSION_DENIED', context.user, { 
  required: 'admin', 
  operation: 'deleteUser' 
});
```

## Configuration for Your Environment

Based on your current setup, ensure your GraphQL server at `http://host.docker.internal:8088/graphql` implements:

1. **JWT validation** against `https://monovandi.eu.auth0.com/`
2. **Audience validation** for `https://api.it-ops.ai`
3. **Permission checking** based on token claims
4. **Proper error handling** for authentication/authorization failures

## Monitoring and Debugging

### 1. Health Check Integration

```javascript
// Add Auth0 connectivity check
app.get('/health', async (req, res) => {
  try {
    // Test JWKS endpoint
    const response = await fetch('https://monovandi.eu.auth0.com/.well-known/jwks.json');
    if (!response.ok) throw new Error('JWKS unreachable');
    
    res.json({
      status: 'healthy',
      auth0: 'connected',
      timestamp: new Date().toISOString()
    });
  } catch (error) {
    res.status(503).json({
      status: 'unhealthy',
      auth0: 'disconnected',
      error: error.message
    });
  }
});
```

### 2. Metrics Collection

```javascript
// Collect authentication metrics
const authMetrics = {
  successful_validations: 0,
  failed_validations: 0,
  permission_denials: 0
};

// In JWT middleware
function recordMetric(type) {
  authMetrics[type]++;
}

// Metrics endpoint
app.get('/metrics', (req, res) => {
  res.json(authMetrics);
});
```

This integration guide provides everything needed to properly handle JWT tokens from the Apollo MCP Server Phase 2 Auth0 implementation in your GraphQL backend.