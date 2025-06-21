// 导入语句
import { useState, useEffect } from 'react';
import axios from 'axios';
import * as _ from 'lodash';
import { User, TaskStatus, UserID, Callback, UserService } from './types';

// 函数定义
function greetUser(user: User): string {
    return `Hello, ${user.name}!`;
}

// 箭头函数
const calculateAge = (birthYear: number): number => {
    const currentYear = new Date().getFullYear();
    return currentYear - birthYear;
};

// 泛型函数
function getProp<T, K extends keyof T>(obj: T, key: K): T[K] {
    return obj[key];
}

// 异步函数
async function fetchUserData(userId: UserID, svc: UserService): Promise<User | null> {
    try {
        const response = await axios.get(`https://api.example.com/users/${userId}`);
        return response.data as User;
    } catch (error) {
        console.error(`Error fetching user ${userId}:`, error);
        return null;
    }
}

// 使用示例
const main = async () => {
    // 使用接口
    const newUser: User = {
        id: 1,
        name: 'John Doe',
        email: 'john@example.com',
        role: 'user'
    };

    // 使用类
    const userService = new UserService('https://api.example.com');
    const users = await userService.getUsers();

    // 使用过滤方法
    const activeUsers = UserService.filterUsers(users, user => user.age !== undefined && user.age > 18);

    // 使用普通函数
    console.log(greetUser(newUser));

    // 使用泛型函数
    const userName = getProp(newUser, 'name');
    console.log(`User name: ${userName}`);

    // 使用异步函数
    const userData = await fetchUserData(1);

    // 使用条件和循环
    if (userData) {
        console.log(`Found user: ${userData.name}`);
    } else {
        console.log('User not found');
    }

    // 使用数组方法
    const names = users.map(user => user.name);
    console.log(`All users: ${names.join(', ')}`);
};

// 调用主函数
main().catch(console.error);