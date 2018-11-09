FROM node:11.1.0-alpine

RUN mkdir -p /usr/src/app
WORKDIR /usr/src/app

ARG NODE_ENV
ENV NODE_ENV $NODE_ENV
COPY package.json /usr/src/app/
RUN yarn --ignore-engines; yarn cache clean
COPY . /usr/src/app

CMD [ "yarn", "start" ]
