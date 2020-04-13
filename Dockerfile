FROM node:13.12.0-slim

RUN mkdir -p /usr/src/app
WORKDIR /usr/src/app

ARG NODE_ENV
ENV NODE_ENV $NODE_ENV
COPY package.json /usr/src/app/
RUN yarn --ignore-engines; yarn cache clean
COPY . /usr/src/app

CMD [ "yarn", "start" ]
